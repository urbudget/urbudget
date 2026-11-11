"""Host-only invariants for the physical-device App Review walkthrough."""
from __future__ import annotations

import json
import os
import re
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from decimal import Decimal
from pathlib import Path


class ReviewError(RuntimeError):
    """An expected stop; messages must never contain credentials or UI dumps."""


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def private_json(path: Path, value) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temporary = path.with_suffix(path.suffix + ".tmp")
    with os.fdopen(os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), "w") as out:
        json.dump(value, out, indent=2)
        out.write("\n")
        out.flush()
        os.fsync(out.fileno())
    os.replace(temporary, path)


def redact(value, secrets=()):
    """Defense in depth for curated events, never a license to log page source."""
    if isinstance(value, dict):
        return {k: "[redacted]" if re.search(r"code|token|secret|passphrase|password|api.?key|email", k, re.I)
                else redact(v, secrets) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [redact(v, secrets) for v in value]
    if isinstance(value, str):
        for secret in sorted((s for s in secrets if s), key=len, reverse=True):
            value = value.replace(secret, "[redacted]")
        return re.sub(r"[\w.+-]+@[\w.-]+\.[A-Za-z]{2,}", "[redacted email]", value)
    return value


def minor(text: str, currency: str) -> int:
    """Parse observed English build-1 money, rejecting ambiguous/localized formats."""
    text = text.replace("\u2212", "-").replace("\u00a0", " ").strip()
    match = re.fullmatch(r"([+-]?(?:\d{1,3}(?:,\d{3})+|\d+)\.\d{2}) " + re.escape(currency), text)
    if not match:
        raise ReviewError("Unrecognized money format; use the observed English UI and currency.")
    return int(Decimal(match[1].replace(",", "")) * 100)


@dataclass(frozen=True)
class Balances:
    assigned: int
    available: int
    unassigned: int
    working: int

    def after_assignment(self):
        if self.unassigned < 5000:
            raise ReviewError("The sample budget needs at least 50.00 unassigned.")
        return Balances(self.assigned + 5000, self.available + 5000, self.unassigned - 5000, self.working)

    def after_outflow(self):
        return Balances(self.assigned, self.available - 1250, self.unassigned, self.working - 1250)

    def verify(self, observed: Balances):
        if self != observed:
            raise ReviewError("Displayed money deltas do not match the recorded baseline.")


def deletion_guard(selected: str, signed_in: str, confirmation: str, *, enabled: bool):
    addresses = [v.strip().casefold() for v in (selected, signed_in, confirmation)]
    if not enabled or not re.fullmatch(r"[^\s@]+@[^\s@]+\.[^\s@]+", addresses[0]):
        raise ReviewError("Disposable-account deletion is disabled or unconfigured.")
    # Also reject plus-addressed variants of the protected review identity.
    if any(v.split("@", 1)[0].split("+", 1)[0] == "review" and v.endswith("@urbudget.app") for v in addresses):
        raise ReviewError("The review account must never be deleted.")
    if len(set(addresses)) != 1:
        raise ReviewError("Selected, signed-in, and confirmation identities must match.")


class Journal:
    """Persist intent before mutation; an uncertain response never permits a blind retry."""
    def __init__(self, path: Path):
        self.path = path
        self.data = json.loads(path.read_text()) if path.exists() else {
            "created_at": now(), "actions": {}, "stages": {}, "events": []}

    def save(self):
        private_json(self.path, self.data)

    def event(self, state: str, **details):
        self.data["events"].append({"at": now(), "state": state, **redact(details)})
        self.save()

    def once(self, name: str, action, verified):
        previous = self.data["actions"].get(name)
        if previous:
            if verified():
                self.data["actions"][name] = "confirmed"
                self.save()
                return
            raise ReviewError("A previous mutation needs inspection. No automatic retry was attempted.")
        self.data["actions"][name] = "intent-recorded"
        self.save()
        action()
        if not verified():
            raise ReviewError("Mutation outcome is unconfirmed. Inspect the phone before continuing.")
        self.data["actions"][name] = "confirmed"
        self.save()

    def baseline(self, balances: Balances):
        if "baseline" in self.data:
            raise ReviewError("This run already has a baseline. Resume its stages or use a fresh sample budget.")
        self.data["baseline"] = asdict(balances)
        self.save()
