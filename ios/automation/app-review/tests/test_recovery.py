import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from safety import Journal
from walkthrough import Walkthrough


class RecoveryTests(unittest.TestCase):
    def test_uncertain_assistant_send_inspects_existing_reply_without_replacing_conversation(self):
        with tempfile.TemporaryDirectory() as temp:
            journal = Journal(Path(temp) / "journal.json")
            journal.data["actions"]["assistant-question"] = "intent-recorded"
            taps = []
            ui = SimpleNamespace(more_menu=lambda: None,
                prefix=lambda _: SimpleNamespace(click=lambda: None),
                shown=lambda name: name == "Report reply", driver=object(),
                tap=taps.append, text=lambda _: None, button=lambda _: None,
                result_pause=lambda: None)
            runner = Walkthrough.__new__(Walkthrough)
            runner.ui, runner.j = ui, journal
            runner.assistant()
            self.assertNotIn("New conversation", taps)
            self.assertNotIn("Send message", taps)
            self.assertEqual(journal.data["actions"]["assistant-question"], "confirmed")

    def test_registered_disposable_identity_is_verified_without_another_code_request(self):
        with tempfile.TemporaryDirectory() as temp:
            journal = Journal(Path(temp) / "journal.json")
            journal.data["actions"]["verify-disposable-code"] = "intent-recorded"
            runner = Walkthrough.__new__(Walkthrough)
            runner.j = journal
            runner.config = {"disposable_email": "sample@example.invalid"}
            runner.account_screen = lambda: None
            runner.identity_is = lambda email: email == runner.config["disposable_email"]
            runner.registration()
            self.assertEqual(journal.data["actions"]["verify-disposable-code"], "confirmed")
            self.assertIn("disposable_fingerprint", journal.data)


if __name__ == "__main__":
    unittest.main()
