"""Run bounded build-1 walkthrough stages, with explicit private/physical handoffs."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import traceback
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo

from appium.webdriver.common.appiumby import AppiumBy
from selenium.webdriver.support.ui import WebDriverWait

from native_recording import NativeRecording
from safety import Balances, Journal, ReviewError, deletion_guard, minor, now, private_json
from session import BUNDLE, DEFAULT_ROOT, Session
from verify_video import REQUIRED

PAYEE = "Review Cafe"
MEMO = "App Review demonstration"
QUESTION = "Summarize this sample budget and explain how much is available to spend."
SAMPLE_STAGES = {"baseline", "assign", "outflow", "reports"}


def handoff(instruction):
    print("\nPHONE HANDOFF: " + instruction, flush=True)
    answer = input("Enter done only after checking the result, or stop to leave this stage incomplete: ").strip().lower()
    if answer != "done":
        raise ReviewError("Owner handoff not completed; this stage remains incomplete.")


class Walkthrough:
    def __init__(self, session, journal, *, allow_delete=False, code_provider=None, revocation_check=None):
        self.s, self.ui, self.j = session, session.ui, journal
        self.config, self.allow_delete = session.config, allow_delete
        self.code_provider, self.revocation_check = code_provider, revocation_check
        self.currency = self.config.get("currency", "SAR")
        self.category = self.config.get("category", "Groceries")
        self.account = self.config.get("account", "Checking")
        self.month = datetime.now(ZoneInfo("Asia/Riyadh")).strftime("%B %Y")
        if not re.fullmatch(r"[A-Z]{3}", self.currency):
            raise ReviewError("Configure the currency displayed by the English sample budget.")

    def labels(self):
        return [e.get("label", "") for e in self.ui.visible()]

    def register(self):
        self.ui.tab("Accounts")
        if not self.ui.shown("Working balance", "XCUIElementTypeStaticText"):
            self.ui.prefix(self.account + ", ").click()
        self.ui.text(self.account)
        self.ui.text("Cash")
        self.ui.text("Working balance")

    def balances(self):
        self.ui.tab("Budget")
        self.ui.button("Choose month, " + self.month)
        assigned = self.ui.prefix(f"Assign to {self.category}, ").get_attribute("label").split(", ", 1)[1]
        available = self.ui.prefix(f"Move money for {self.category}, ").get_attribute("label").split(", ", 1)[1].removesuffix(" available")
        unassigned = self.ui.prefix("Unassigned ").get_attribute("label").removeprefix("Unassigned ").removesuffix(". Assign money")
        self.register()
        tree = self.ui.tree()
        working = []
        for parent in tree.iter():
            children = list(parent)
            for i, element in enumerate(children[:-1]):
                if element.get("label") == "Working balance" and element.get("visible") == "true":
                    working.append(minor(children[i + 1].get("label", ""), self.currency))
        if len(working) != 1:
            raise ReviewError("Could not uniquely identify the account's working balance.")
        self.ui.tab("Budget")
        return Balances(minor(assigned, self.currency), minor(available, self.currency), minor(unassigned, self.currency), working[0])

    def transaction_count(self):
        self.ui.tab("Transactions")
        # Fresh samples have a small register; refuse any existing demonstration.
        return len(self.demonstration_rows())

    def demonstration_rows(self):
        # The cleared seed row is StaticText; a new uncleared row is Other.
        # Count accessible grouped rows, excluding their inaccessible child labels.
        return [e.get("label", "") for e in self.ui.visible()
                if e.get("label", "").startswith(PAYEE + ", ") and MEMO in e.get("label", "")]

    def baseline(self):
        if not self.config.get("sample_budget_confirmed_by_owner") or not self.config.get("sample_budget_name"):
            raise ReviewError("Confirm a fresh disposable sample budget and configure its visible name.")
        self.ui.activate(BUNDLE)
        self.ui.more_menu()
        name = self.config["sample_budget_name"]
        self.ui.one('type == "XCUIElementTypeButton" AND label CONTAINS ' + json.dumps(", " + name + ", " + self.currency + " · Switch or manage budgets"))
        if self.transaction_count() != 0:
            raise ReviewError("This sample already contains a demonstration transaction; prepare a fresh sample.")
        before = self.balances()
        before.after_assignment()  # Check unassigned funds before any edit.
        self.ui.reports_menu()
        self.ui.button(self.month)
        spending = self.ui.prefix(self.category + ", ").get_attribute("label").removeprefix(self.category + ", ")
        self.j.data.update({"currency": self.currency, "sample_budget_name": name, "category": self.category,
                           "account": self.account, "month": self.month,
                           "spending_baseline": minor(spending, self.currency), "day": datetime.now(ZoneInfo("Asia/Riyadh")).date().isoformat()})
        self.j.baseline(before)
        self.ui.tab("Budget")

    def launch(self):
        self.ui.activate("com.apple.springboard")
        self.ui.driver.execute_script("mobile: pressButton", {"name": "home"})
        icon = self.config.get("verified_home_icon")
        if icon:
            # Observed and bundle-checked outside capture; never choose the first
            # of two identically labelled apps. Reject changed layouts.
            for _ in range(icon["swipes_left"]):
                self.ui.scroll("left")
            self.ui.one('type == "XCUIElementTypeIcon" AND label == "UrBudget"'
                        f' AND rect.x == {int(icon["x"])} AND rect.y == {int(icon["y"])}').click()
        else:
            handoff("Tap the installed TestFlight UrBudget icon on the Home Screen. Leave the sample Budget tab visible.")
        self.ui.active(BUNDLE)
        self.ui.tab("Budget")

    def expected(self, spent=False):
        if "baseline" not in self.j.data:
            raise ReviewError("Run baseline on the fresh sample before editing it.")
        expected = Balances(**self.j.data["baseline"]).after_assignment()
        return expected.after_outflow() if spent else expected

    def assign(self):
        expected = self.expected()
        def verify():
            # A stage rerun starts on normal tabs; a successful Assign leaves the
            # category sheet open. Close it only after Assign has left edit mode.
            if self.ui.shown("Close"):
                if self.ui.shown("Assign"):
                    return False
                self.ui.tap("Close")
            expected.verify(self.balances())
            return True
        def action():
            Balances(**self.j.data["baseline"]).verify(self.balances())
            self.ui.prefix(f"Assign to {self.category}, ").click()
            self.ui.button("Assign")
            # Build 1 starts this keypad fresh: digits replace the absolute total.
            self.ui.digits(expected.assigned)
            shown = self.ui.prefix("Assigned in ").get_attribute("label").split(", ", 1)[1]
            if minor(shown, self.currency) != expected.assigned:
                raise ReviewError("The assignment draft is not the expected absolute amount; cancel it before continuing.")
            self.ui.tap("Assign")
            WebDriverWait(self.ui.driver, 20).until(lambda _: not self.ui.shown("Assign"))
        self.j.once("assign-50", action, verify)

    def outflow(self):
        if self.j.data.get("day") != datetime.now(ZoneInfo("Asia/Riyadh")).date().isoformat():
            raise ReviewError("The day changed; prepare a fresh take with today's date.")
        def verify():
            WebDriverWait(self.ui.driver, 20).until(lambda _: not self.ui.shown("Save"))
            if self.transaction_count() != 1:
                raise ReviewError("Expected exactly one demonstration transaction in the register.")
            self.expected(spent=True).verify(self.balances())
            return True
        def action():
            self.expected().verify(self.balances())
            if self.transaction_count() != 0:
                raise ReviewError("A demonstration already exists; refusing another Save.")
            self.ui.tap("Add transaction")
            self.ui.button("Outflow")
            self.ui.digits(1250)
            self.ui.tap("Done")  # Opens the payee picker for a new transaction.
            search = self.ui.field("Search or add a payee", "SearchField")
            search.send_keys(PAYEE)
            self.ui.tap("Add “" + PAYEE + "”")
            # Selecting a new payee advances directly to Category in build 1.
            self.ui.text("Category")
            self.ui.prefix(self.category + ", ").click()
            self.ui.button("Payee, " + PAYEE)
            self.ui.button("Category, " + self.category)
            self.ui.button("Account, " + self.account)
            self.ui.button("Amount, 12.50 " + self.currency + ". Edit amount")
            self.ui.text("Date · Today")
            self.ui.button("Repeat, Never")
            memo = self.ui.field("Memo")
            memo.click()
            memo.send_keys(MEMO)
            self.ui.field(MEMO)  # Re-find after SwiftUI replaces the field node.
            self.ui.tap("Save")
        self.j.once("save-outflow-12.50", action, verify)

    def reports(self):
        self.expected(spent=True).verify(self.balances())
        self.register()
        rows = self.demonstration_rows()
        if len(rows) != 1 or "-12.50 " + self.currency not in rows[0] or self.category not in rows[0]:
            raise ReviewError("The selected account must contain exactly the expected transaction.")
        self.ui.result_pause()
        self.ui.reports_menu()
        self.ui.button(self.month)
        total = self.ui.prefix(self.category + ", ").get_attribute("label").removeprefix(self.category + ", ")
        if minor(total, self.currency) != self.j.data["spending_baseline"] + 1250:
            raise ReviewError("The spending report did not include the new outflow.")
        self.ui.tap("Spending breakdown")
        self.ui.tap("Everyday")
        self.ui.tap(self.category)
        day = self.j.data["day"]
        self.ui.button(f"{PAYEE}, {day} · {self.account} · {self.category}, -12.50 {self.currency}")
        ranges = [label for label in self.labels() if re.fullmatch(r"\d{4}-\d{2}-\d{2} – \d{4}-\d{2}-\d{2}", label)]
        if len(ranges) != 1 or not ranges[0][:10] <= day <= ranges[0][-10:]:
            raise ReviewError("The report date range does not include the demonstration date.")
        self.ui.result_pause()
        self.ui.reports_menu()

    def account_screen(self):
        self.ui.more_menu()
        self.ui.prefix("Account & sync,").click()
        self.ui.text("Account & sync")

    def identity_is(self, email):
        # Read labels only after the owner has completed sign-in. Never read field
        # values, codes, provider configuration or passphrases for diagnostics.
        if not self.ui.shown("Sign out on this device"):
            return False
        predicate = 'visible == true AND (label == ' + json.dumps(email) + ' OR label BEGINSWITH ' + json.dumps(email + ",") + ')'
        return bool(self.ui.driver.find_elements(AppiumBy.IOS_PREDICATE, predicate))

    def review_access(self):
        self.account_screen()
        if not self.identity_is("review@urbudget.app"):
            handoff("Sign in as review@urbudget.app using its private review code on the phone. Leave Account & sync visible. Do not put the code in this terminal.")
        if not self.identity_is("review@urbudget.app"):
            raise ReviewError("The signed-in identity is not the protected review account.")
        self.ui.text("Sync subscription active")
        self.j.event("review-entitlement-verified", login_demonstration="ordinary email-code flow in registration stage")

    def sync(self):
        self.account_screen()
        if not self.identity_is("review@urbudget.app"):
            raise ReviewError("Sync demonstration requires the review account.")
        if not self.ui.shown("Sync now"):
            handoff("Set up or unlock Sync privately, retaining the passphrase for Apple's private access instructions. Enroll only this disposable sample budget. Do not reset an existing passphrase. Return to Account & sync.")
        self.ui.button("Sync now")
        if self.ui.sync_state(self.config["sample_budget_name"]) == "This device only":
            self.j.once("enroll-selected-sample", lambda: self.ui.start_sample_sync(self.config["sample_budget_name"]),
                        lambda: bool(WebDriverWait(self.ui.driver, 40).until(
                            lambda _: self.ui.sync_state(self.config["sample_budget_name"]) in {"Synced", "Uploaded"})))
        def verified():
            WebDriverWait(self.ui.driver, 40).until(
                lambda _: self.ui.sync_state(self.config["sample_budget_name"]) == "Synced")
            return True
        self.j.once("sync-now", lambda: self.ui.tap("Sync now"), verified)
        self.j.event("sample-sync-status-verified", cloud_download="not verified; no second device")

    def assistant(self):
        self.ui.more_menu()
        self.ui.prefix("Assistant,").click()
        if self.ui.shown("Set up assistant"):
            handoff("Configure a working provider privately in Assistant settings, then return to the conversation. Do not enter API keys in the terminal.")
        if "assistant-question" not in self.j.data["actions"]:
            self.ui.tap("New conversation")
            self.ui.field("Ask about your budget").send_keys(QUESTION)
            # Multiline input resizes the composer while the keyboard settles.
            # Give viewers time to read it, then resolve the button's fresh bounds.
            self.ui.result_pause()
        def verified():
            WebDriverWait(self.ui.driver, 180).until(lambda _: self.ui.shown("Report reply") and not self.ui.shown("Stop reply"))
            return True
        def send():
            self.ui.tap("Send message")
            # Distinguish provider latency from a tap that left the draft unsent.
            predicate = 'type == "XCUIElementTypeTextField" AND value == ' + json.dumps(QUESTION) + ' AND visible == true'
            WebDriverWait(self.ui.driver, 20).until(lambda _: self.ui.shown("Stop reply")
                or not self.ui.driver.find_elements(AppiumBy.IOS_PREDICATE, predicate))
            self.j.event("assistant-send-accepted")
        self.j.once("assistant-question", send, verified)
        self.ui.result_pause()
        self.ui.tap("Report reply")
        self.ui.text("Report a reply")
        self.ui.button("Send report")
        self.ui.result_pause()
        self.ui.tap("Cancel")  # Never submit a false report.
        self.ui.tap("Done")

    def registration(self):
        if not self.config.get("disposable_email"):
            raise ReviewError("Configure the explicitly selected unused disposable email first.")
        if ("request-disposable-code" not in self.j.data["actions"]
                or self.j.data["actions"].get("verify-disposable-code") == "confirmed"):
            self.account_screen()
        if self.identity_is(self.config["disposable_email"]) and "verify-disposable-code" in self.j.data["actions"]:
            self.j.once("verify-disposable-code", lambda: None,
                        lambda: self.identity_is(self.config["disposable_email"]))
            self.j.data["disposable_fingerprint"] = hashlib.sha256(self.config["disposable_email"].strip().lower().encode()).hexdigest()
            self.j.save()
            return
        if self.code_provider:
            if self.ui.shown("Sign out on this device"):
                if not self.identity_is("review@urbudget.app"):
                    raise ReviewError("Expected the protected review account before disposable registration.")
                self.ui.tap("Sign out on this device")
                self.ui.tap("Sign out")
            if "request-disposable-code" not in self.j.data["actions"]:
                field = self.ui.field("Email address")
                field.click()
                field.send_keys(self.config["disposable_email"])
                self.ui.result_pause()
                self.ui.finish_email_entry()
                self.j.data["registration_requested_at"] = now()
                self.j.save()
            requested = self.j.data.get("registration_requested_at")
            if not requested:
                raise ReviewError("Inspect the pending code request and retain its request time before resuming.")
            self.j.once("request-disposable-code", lambda: self.ui.tap("Send sign-in code"),
                        lambda: bool(self.ui.field("Code from the email")))
            code = self.code_provider(self.config["disposable_email"], requested)
            if not re.fullmatch(r"\d{6}", code):
                raise ReviewError("Expected a fresh six-digit disposable-account email code.")
            field = self.ui.field("Code from the email")
            field.click()
            field.send_keys(code)
            del code
            self.ui.result_pause()
            self.j.once("verify-disposable-code", lambda: self.ui.tap("Sign in"),
                        lambda: bool(WebDriverWait(self.ui.driver, 30).until(
                            lambda _: self.identity_is(self.config["disposable_email"]))))
        else:
            handoff("Sign out of the review account. Register the selected unused disposable address through its normal email-code flow on the phone, keeping the inbox outside the recording. Leave Account & sync visible.")
        if not self.identity_is(self.config["disposable_email"]):
            raise ReviewError("The signed-in identity does not match the selected disposable address.")
        self.j.data["disposable_fingerprint"] = hashlib.sha256(self.config["disposable_email"].strip().lower().encode()).hexdigest()
        self.j.save()

    def deletion(self):
        self.account_screen()
        selected = self.config.get("disposable_email", "")
        signed_in = selected if self.identity_is(selected) else ""
        deletion_guard(selected, signed_in, selected, enabled=self.allow_delete)
        self.ui.reveal("Delete account and cloud data").click()
        self.ui.text("Delete account")
        # --allow-disposable-deletion is explicit authorization for this named
        # disposable test account; three identity checks still guard the tap.
        self.ui.field("Type " + selected + " to confirm").send_keys(selected)
        # Guard the exact field contents before the single destructive tap.
        confirm = self.ui.field(selected).get_attribute("value")
        deletion_guard(selected, signed_in, confirm, enabled=self.allow_delete)
        def verified():
            self.ui.field("Email address")
            if self.ui.shown("Sign out on this device"):
                raise ReviewError("Account deletion did not return to the signed-out state.")
            self.expected(spent=True).verify(self.balances())
            if self.revocation_check:
                if not self.revocation_check():
                    raise ReviewError("The old disposable session has not been verified as revoked.")
                self.j.data["old_session_revoked"] = True
                self.j.event("old-disposable-session-revoked")
            else:
                self.j.event("old-session-revocation-unverified")
            return True
        self.j.once("delete-selected-disposable", lambda: self.ui.tap("Delete"), verified)

    def run_stage(self, name):
        started = now()
        self.j.event("stage-started", stage=name)
        try:
            getattr(self, name)()
            self.ui.result_pause()
            self.j.data["stages"][name] = {"status": "passed", "started_at": started, "at": now(),
                "verification": "UI assertions; manual handoffs where configured"}
            self.j.event("stage-passed", stage=name)
        except BaseException as error:
            frames = [{"file": Path(frame.filename).name, "line": frame.lineno, "function": frame.name}
                      for frame in traceback.extract_tb(error.__traceback__)
                      if Path(frame.filename).resolve().parent == Path(__file__).resolve().parent]
            self.j.data["stages"][name] = {"status": "incomplete", "started_at": started, "at": now(),
                                           "error_type": type(error).__name__, "harness_frames": frames}
            self.j.event("stage-stopped", stage=name, error_type=type(error).__name__)
            # No source dumps or exception strings: WDA errors can contain the full UI tree.
            # A failed navigation may leave an account/credential form visible.
            # Capture only after identifying a normal sample tab with no editor.
            if name in SAMPLE_STAGES and "baseline" in self.j.data:
                try:
                    fields = self.ui.driver.find_elements(AppiumBy.IOS_PREDICATE,
                        'visible == true AND (type == "XCUIElementTypeTextField" OR type == "XCUIElementTypeSecureTextField")')
                    sample_tab = self.ui.driver.find_elements(AppiumBy.IOS_PREDICATE,
                        'type == "XCUIElementTypeTabBar" AND visible == true')
                    selected_more = self.ui.driver.find_elements(AppiumBy.IOS_PREDICATE,
                        'type == "XCUIElementTypeButton" AND label == "More" AND value == "1" AND visible == true')
                    if sample_tab and not fields and not selected_more:
                        self.ui.driver.save_screenshot(str(self.s.evidence / (name + "-stopped.png")))
                except Exception:
                    pass
            raise


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--config", type=Path, default=DEFAULT_ROOT / "private.json")
    p.add_argument("--run", required=True, help="A new simple name for each fresh sample/take; reuse only to inspect/resume")
    p.add_argument("--stage", choices=("all", "baseline", *REQUIRED), default="all")
    p.add_argument("--mode", choices=("rehearsal", "final"), default="rehearsal")
    p.add_argument("--rehearsal", type=Path, help="Passed rehearsal journal required for final capture")
    p.add_argument("--manual-recording", action="store_true")
    p.add_argument("--capture-rehearsal", action="store_true", help="Retain native rehearsal footage; never certifies it as a final take")
    p.add_argument("--otp-file", type=Path, help="Private, fresh code receipt from the explicitly selected mailbox; consumed once")
    p.add_argument("--allow-disposable-deletion", action="store_true", help="Authorize deletion of the selected disposable test account after matching identities")
    args = p.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,80}", args.run):
        raise ReviewError("Use a simple run name with no path separators.")
    if args.mode == "final" and args.stage != "all":
        raise ReviewError("Final capture must run the complete sequence with a new sample budget.")
    if args.capture_rehearsal and (args.mode != "rehearsal" or args.stage != "all"):
        raise ReviewError("Recorded rehearsals require rehearsal mode and the complete sequence.")
    root = DEFAULT_ROOT / "evidence" / args.run
    journal = Journal(root / "journal.json")
    if args.mode == "final":
        if journal.path.exists():
            raise ReviewError("Final capture needs a new run directory and fresh sample budget.")
        if not args.rehearsal:
            raise ReviewError("Provide the successful rehearsal journal first.")
        previous = json.loads(args.rehearsal.read_text())
        if any(previous.get("stages", {}).get(s, {}).get("status") != "passed" for s in REQUIRED):
            raise ReviewError("All rehearsal stages must pass before final capture.")
        if not previous.get("old_session_revoked"):
            raise ReviewError("The rehearsal must also verify revocation of an independent old session.")
        selected = json.loads(args.config.read_text()).get("disposable_email", "")
        if hashlib.sha256(selected.strip().lower().encode()).hexdigest() == previous.get("disposable_fingerprint"):
            raise ReviewError("Final registration requires another unused disposable address.")
    with Session(args.config, root) as session:
        session.verify_testflight()
        journal.data.update({"identity": session.identity, "mode": args.mode})
        journal.save()
        from private_code import code_from_file
        provider = (lambda email, requested: code_from_file(args.otp_file, email, requested)) if args.otp_file else None
        walkthrough = Walkthrough(session, journal, allow_delete=args.allow_disposable_deletion, code_provider=provider)
        recorder = NativeRecording(session.ui, journal, args.manual_recording)
        stages = ["baseline", *REQUIRED] if args.stage == "all" else [args.stage]
        try:
            for stage in stages:
                if (args.mode == "final" or args.capture_rehearsal) and stage == "launch":
                    if args.manual_recording:
                        handoff("Prepare Focus and the Home Screen, close mirroring, and confirm you are ready for the native take.")
                    recorder.start(handoff)
                walkthrough.run_stage(stage)
        except BaseException:
            capture = journal.data.get("recording", {})
            if capture.get("started_at") and not capture.get("stopped_at") and not args.manual_recording:
                try:
                    recorder.stop(handoff, confirm_photos=False)
                except Exception as error:
                    journal.event("automatic-capture-stop-unconfirmed", error_type=type(error).__name__)
            raise
        if args.mode == "final" or args.capture_rehearsal:
            recorder.stop(handoff)
        print("Requested stages completed. Export and verify the native original separately.")


if __name__ == "__main__":
    try:
        main()
    except (Exception, KeyboardInterrupt) as error:
        # Intentionally do not print arbitrary WebDriver error text or tracebacks.
        message = str(error) if isinstance(error, ReviewError) else type(error).__name__
        print("Stopped: " + message + ". Inspect the journal and phone; do not repeat a mutation blindly.", file=sys.stderr)
        print("If native recording is still running, stop it on the phone and retain the clip.", file=sys.stderr)
        sys.exit(1)
