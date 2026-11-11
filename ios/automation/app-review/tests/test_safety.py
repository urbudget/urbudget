import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from safety import Balances, Journal, ReviewError, deletion_guard, minor, redact
from session import capabilities, validate_identity
from verify_video import REQUIRED, coverage


class MoneyTests(unittest.TestCase):
    def test_working_balance_and_net_available(self):
        before = Balances(60000, 47500, 320000, 487500)
        assigned = before.after_assignment()
        self.assertEqual(assigned, Balances(65000, 52500, 315000, 487500))
        after = assigned.after_outflow()
        self.assertEqual(after, Balances(65000, 51250, 315000, 486250))
        self.assertEqual(after.available - before.available, 3750)
        with self.assertRaises(ReviewError):
            after.verify(Balances(65000, 51250, 315000, 487500))  # Cleared-only balance is wrong.

    def test_reject_insufficient_funds_and_ambiguous_numbers(self):
        with self.assertRaises(ReviewError):
            Balances(0, 0, 4999, 50000).after_assignment()
        for value in ("1.234,56 SAR", "1,23.45 SAR", "12.5 SAR", "12.50 USD", "NaN SAR"):
            with self.subTest(value=value), self.assertRaises(ReviewError):
                minor(value, "SAR")
        self.assertEqual(minor("−1,234.56\u00a0SAR", "SAR"), -123456)


class MutationTests(unittest.TestCase):
    def test_uncertain_save_is_not_repeated_after_restart(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "journal.json"
            count = 0
            def save_with_lost_response():
                nonlocal count
                count += 1
                raise TimeoutError("response lost after Save")
            with self.assertRaises(TimeoutError):
                Journal(path).once("save", save_with_lost_response, lambda: False)
            self.assertEqual(json.loads(path.read_text())["actions"]["save"], "intent-recorded")
            with self.assertRaises(ReviewError):
                Journal(path).once("save", save_with_lost_response, lambda: False)
            recovered = Journal(path)
            recovered.once("save", save_with_lost_response, lambda: True)
            self.assertEqual(count, 1)
            self.assertEqual(recovered.data["actions"]["save"], "confirmed")
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_successful_mutation_is_not_repeated(self):
        with tempfile.TemporaryDirectory() as temp:
            journal = Journal(Path(temp) / "journal.json")
            calls = []
            journal.once("sync", lambda: calls.append("sync"), lambda: True)
            journal.once("sync", lambda: calls.append("sync"), lambda: True)
            self.assertEqual(calls, ["sync"])

    def test_baseline_cannot_be_replaced(self):
        with tempfile.TemporaryDirectory() as temp:
            journal = Journal(Path(temp) / "journal.json")
            journal.baseline(Balances(1, 2, 3, 4))
            with self.assertRaises(ReviewError):
                Journal(journal.path).baseline(Balances(5, 6, 7, 8))


class DeletionTests(unittest.TestCase):
    def test_only_explicit_matching_disposable_identity(self):
        email = "sample@example.invalid"
        deletion_guard(email, email.upper(), " " + email, enabled=True)
        for selected, signed, confirm, enabled in (
            (email, email, email, False), ("", "", "", True),
            (email, "other@example.invalid", email, True), (email, email, "different@example.invalid", True),
            ("review@urbudget.app", "review@urbudget.app", "review@urbudget.app", True),
            ("REVIEW+test@urbudget.app", "REVIEW+test@urbudget.app", "REVIEW+test@urbudget.app", True),
            (email, "review@urbudget.app", email, True),
        ):
            with self.subTest(selected=selected, enabled=enabled), self.assertRaises(ReviewError):
                deletion_guard(selected, signed, confirm, enabled=enabled)

    def test_curated_events_redact_nested_credentials(self):
        result = redact({"stage": "login", "email": "a@example.invalid", "details": [
            {"apiKey": "private", "message": "do not log customsecret or a@example.invalid"}],
            "passphrase": "private"}, secrets=("customsecret",))
        serialized = json.dumps(result)
        for secret in ("private", "customsecret", "a@example.invalid"):
            self.assertNotIn(secret, serialized)
        self.assertEqual(result["stage"], "login")


class IdentityTests(unittest.TestCase):
    def setUp(self):
        self.udid = "00000000-0000000000000000"
        self.device = {"hardwareProperties": {"udid": self.udid, "reality": "physical", "platform": "iOS"},
                       "deviceProperties": {"osVersionNumber": "27.2", "developerModeStatus": "enabled"}}
        self.apps = {"apps": [{"bundleIdentifier": "app.urbudget", "version": "1.0.0", "bundleVersion": "1", "builtByDeveloper": True}]}
        self.config = {"udid": self.udid, "team_id": "AAAAAAAAAA", "wda_bundle_id": "app.urbudget.review.WebDriverAgentRunner"}

    def test_valid_identity_still_requires_testflight_evidence(self):
        identity = validate_identity(self.device, self.apps, self.udid)
        self.assertEqual(identity["distribution"], "requires TestFlight UI verification")
        self.assertTrue(identity["builtByDeveloper"])

    def test_reject_simulator_wrong_device_wrong_build_missing_os(self):
        for section, key, value in (("hardwareProperties", "reality", "simulated"),
                                    ("hardwareProperties", "udid", "different"),
                                    ("deviceProperties", "osVersionNumber", None),
                                    ("deviceProperties", "developerModeStatus", "disabled")):
            device = copy.deepcopy(self.device)
            device[section][key] = value
            with self.subTest(key=key), self.assertRaises(ReviewError):
                validate_identity(device, self.apps, self.udid)
        for key, value in (("bundleIdentifier", "app.urbudget.native"), ("version", "1.0.1"), ("bundleVersion", "2")):
            apps = copy.deepcopy(self.apps)
            apps["apps"][0][key] = value
            with self.subTest(key=key), self.assertRaises(ReviewError):
                validate_identity(self.device, apps, self.udid)

    def test_app_cannot_be_replaced_via_config(self):
        caps = capabilities({**self.config, "app": "anything.ipa", "bundle_id": "wrong", "fullReset": True})
        self.assertNotIn("appium:app", caps)
        self.assertTrue(caps["appium:noReset"])
        self.assertFalse(caps["appium:fullReset"])
        self.assertFalse(caps["appium:autoLaunch"])
        self.assertFalse(caps["appium:shouldTerminateApp"])
        self.assertEqual(caps["appium:bundleId"], "app.urbudget")
        for helper in ("app.urbudget", "app.urbudget.native", "com.example.runner"):
            with self.assertRaises(ReviewError):
                capabilities({**self.config, "wda_bundle_id": helper})
        for url in ("https://example.com", "http://8.8.8.8:8100", "http://user:secret@127.0.0.1:8100"):
            with self.assertRaises(ReviewError):
                capabilities({**self.config, "wda_url": url})


class CoverageTests(unittest.TestCase):
    def complete_take(self):
        return {"mode": "final", "identity": {"bundle": "app.urbudget", "version": "1.0.0", "build": "1", "os": "27.2",
                    "distribution": "TestFlight build 1 Open button launched app.urbudget"},
                "recording": {"method": "iPhone built-in Screen Recording", "started_at": "2026-10-31T00:00:00+00:00",
                    "stopped_at": "2026-10-31T00:20:00+00:00", "photos_save_confirmed": True},
                "stages": {name: {"status": "passed", "started_at": f"2026-10-31T00:{i:02d}:01+00:00",
                    "at": f"2026-10-31T00:{i:02d}:30+00:00"} for i, name in enumerate(REQUIRED)}}

    def check(self, journal):
        return coverage(journal, playback_reviewed=True, original_confirmed=True)["coverage_complete"]

    def test_complete_take_requires_human_playback_and_original_match(self):
        journal = self.complete_take()
        self.assertTrue(self.check(journal))
        self.assertFalse(coverage(journal, original_confirmed=True)["coverage_complete"])
        self.assertFalse(coverage(journal, playback_reviewed=True)["coverage_complete"])

    def test_rehearsal_or_unrelated_stages_cannot_certify_video(self):
        for section, key, value in ((None, "mode", "rehearsal"),
                                    ("recording", "photos_save_confirmed", False),
                                    ("recording", "method", "Appium screenshot stream"),
                                    ("recording", "started_at", "2026-10-31T00:10:00+00:00"),
                                    ("identity", "distribution", "owner guessed"),
                                    ("identity", "build", "2")):
            journal = self.complete_take()
            (journal[section] if section else journal)[key] = value
            with self.subTest(key=key):
                self.assertFalse(self.check(journal))
        for name in REQUIRED:
            journal = self.complete_take()
            journal["stages"][name]["status"] = "incomplete"
            self.assertFalse(self.check(journal))
        self.assertFalse(self.check({}))


if __name__ == "__main__":
    unittest.main()
