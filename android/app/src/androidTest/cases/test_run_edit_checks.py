"""run_edit_checks.py's own rules, with adb stubbed, so they need no emulator.

    python -m unittest test_run_edit_checks
"""
import os
import re
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock

import run_edit_checks as checks

KILLED = "INSTRUMENTATION_RESULT: shortMsg=Process crashed.\nINSTRUMENTATION_CODE: 0\n"
TESTS = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "java", "app", "urbudget", "nativeapp"))


class Arm:
    """The arm phase: still editing unless `ended`, and printing `output` however it stops."""
    def __init__(self, output, ended=False):
        self.output, self.ended = output, ended

    def poll(self):
        return 0 if self.ended else None

    def communicate(self, timeout=None):
        return self.output, None

    def kill(self):
        pass


def cycle(arm, logcat=""):
    """One death cycle against `arm`, with verify passing. Returns its result and what it saved."""
    def adb(*args, **kwargs):
        if args[:3] == ("logcat", "-d", "-s"):
            return SimpleNamespace(stdout="armed 1\nverified at step 4, with the last edit")
        if args[:2] == ("logcat", "-d"):
            return SimpleNamespace(stdout=logcat)
        return SimpleNamespace(stdout="OK (1 test)\n", stderr="")
    with tempfile.TemporaryDirectory() as out, mock.patch.object(checks, "adb", adb), \
            mock.patch.object(checks.subprocess, "Popen", return_value=arm), mock.patch.object(checks.time, "sleep"), \
            mock.patch("builtins.print"):
        return checks.death_cycle(1, out), sorted(os.listdir(out))


class DeathCycle(unittest.TestCase):
    def test_a_kill_that_lands_passes_and_keeps_the_logcat(self):
        passed, saved = cycle(Arm(KILLED))
        self.assertTrue(passed)
        self.assertIn("death-1-arm.logcat.txt", saved)
        self.assertIn("death-1-verify.logcat.txt", saved)

    def test_an_arm_that_failed_by_itself_fails(self):
        self.assertFalse(cycle(Arm("FAILURES!!!\nTests run: 1,  Failures: 1\n", ended=True))[0])

    def test_an_arm_that_crashed_before_the_kill_fails(self):
        self.assertFalse(cycle(Arm(KILLED, ended=True))[0])

    def test_a_failure_the_kill_only_just_beat_fails(self):
        self.assertFalse(cycle(Arm("FAILURES!!!\n"))[0])

    def test_a_crash_only_logcat_saw_fails(self):
        self.assertFalse(cycle(Arm(KILLED), "E AndroidRuntime: FATAL EXCEPTION: main\nE AndroidRuntime: Process: app.urbudget.nativeapp, PID: 1\n")[0])
        self.assertFalse(cycle(Arm(KILLED), "F libc: Fatal signal 11 (SIGSEGV), code 1 in tid 9 (DefaultDispatch), pid 9 (rbudget.nativeapp)\n")[0])

    def test_another_apps_crash_doesnt_count(self):
        self.assertTrue(cycle(Arm(KILLED), "E AndroidRuntime: FATAL EXCEPTION: main\nE AndroidRuntime: Process: com.android.systemui, PID: 1\n")[0])


class Skips(unittest.TestCase):
    def test_no_class_can_skip_past_the_strict_check(self):
        for name in checks.CLASSES:
            with open(os.path.join(TESTS, *name.split(".")) + ".kt", encoding="utf-8") as f:
                self.assertIsNone(re.search(r"assumeTrue\(", f.read()), f"{name} has a skip of its own")


if __name__ == "__main__":
    unittest.main()
