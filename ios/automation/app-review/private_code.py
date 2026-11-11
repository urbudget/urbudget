"""Consume a fresh mailbox code receipt without writing credentials to the journal."""
from __future__ import annotations

import json
import stat
import time
from datetime import datetime, timezone
from pathlib import Path

from safety import ReviewError


def validate_receipt(receipt, email, requested_at, current):
    try:
        received = datetime.fromisoformat(receipt["received_at"])
        expires = datetime.fromisoformat(receipt["expires_at"])
        requested = datetime.fromisoformat(requested_at)
        if (receipt["email"].casefold() != email.casefold()
                or not requested <= received <= current < expires
                or (expires - received).total_seconds() > 600
                or not isinstance(receipt["code"], str)
                or len(receipt["code"]) != 6 or not receipt["code"].isascii()
                or not receipt["code"].isdigit()):
            raise ValueError
    except (KeyError, TypeError, ValueError):
        raise ReviewError("The private email-code receipt is expired, mismatched or malformed.") from None
    return receipt["code"]


def code_from_file(path: Path, email: str, requested_at: str, timeout=300):
    reports = Path(__file__).resolve().parents[3] / "reports"
    if path.is_symlink() or not path.resolve().is_relative_to(reports):
        raise ReviewError("Keep the private email-code receipt under ignored reports.")
    deadline = time.monotonic() + timeout
    print("Waiting for the selected account's private mailbox-code receipt.", flush=True)
    while time.monotonic() < deadline:
        if path.exists():
            metadata = path.lstat()
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_mode & 0o077:
                raise ReviewError("The private email-code receipt must be a regular mode-0600 file.")
            try:
                receipt = json.loads(path.read_text())
            except (ValueError, UnicodeError):
                raise ReviewError("The private email-code receipt could not be read.") from None
            code = validate_receipt(receipt, email, requested_at, datetime.now(timezone.utc))
            path.unlink()
            return code
        time.sleep(1)
    raise ReviewError("No fresh email-code receipt arrived; inspect sign-in before retrying.")
