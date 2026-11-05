"""The engine-edit checks, one process per class, on the QA emulator and nowhere else.

    python run_edit_checks.py [--only Class ...] [--deaths 10] [--no-deaths] [--out folder]

Run it after installing the debug and test APKs with adb -s emulator-5584 install. Never use a Gradle
connected*AndroidTest task for this: it reinstalls on every device, and uninstalls afterwards.

A bare exit code proves nothing here: am instrument can exit zero after a failure, and a test skipped
by assumeTrue reads as a pass. So each class must print its exact "OK (n tests)". Each run also runs
under -e strict 1, which turns the emulator check from a skip into a failure.
"""
import argparse
import datetime
import os
import random
import re
import subprocess
import sys
import time

ADB = os.path.expandvars(r"%LOCALAPPDATA%\Android\Sdk\platform-tools\adb.exe") if os.name == "nt" else "adb"
SERIAL = os.environ.get("EDIT_CHECK_SERIAL", "emulator-5584")
if not SERIAL.startswith("emulator-"):
    sys.exit(f"Emulators only, not {SERIAL}")
APP = "app.urbudget.nativeapp"
RUNNER = f"{APP}.test/androidx.test.runner.AndroidJUnitRunner"

# Exact counts. A class that ran fewer tests than this has lost some, and fails.
CLASSES = {
    "BudgetPatchFixtureTest": 3,
    "BudgetViewModelFailureTest": 9,
    "EngineEditEquivalenceTest": 4,
    "EngineEditUndoTest": 16,
    "EngineEditConflictTest": 24,
    "EngineEditLifecycleTest": 13,
    "ui.EngineEditUiTest": 33,
    "ui.SmsImportUiTest": 16,
}
# Run again at a real size, on top of the ordinary run in the full suite.
EXTRAS = {"ui.SmsImportUiTest": ("-e", "large", "1")}
BAD = ("FAILURES!!!", "INSTRUMENTATION_FAILED", "Process crashed", "INSTRUMENTATION_ABORTED", "shortMsg=")
# In a death cycle "Process crashed" is what the force-stop prints too, so a real crash shows only here.
CRASH = re.compile(rf"FATAL EXCEPTION.*\n.*Process: {re.escape(APP)}|Fatal signal.*nativeapp")


def adb(*args, **kwargs):
    return subprocess.run([ADB, "-s", SERIAL, *args], capture_output=True, text=True, encoding="utf-8", errors="replace", **kwargs)


def instrument(*args):
    return ["shell", "am", "instrument", "-w", "-e", "strict", "1", *args, RUNNER]


def stop():
    adb("shell", "am", "force-stop", APP)


def run_class(name, wanted, out):
    stop()
    adb("logcat", "-c")
    began = time.time()
    done = adb(*instrument(*EXTRAS.get(name, ()), "-e", "class", f"{APP}.{name}"))
    took = time.time() - began
    text = done.stdout + done.stderr
    logcat = adb("logcat", "-d").stdout
    with open(os.path.join(out, f"{name}.txt"), "w", encoding="utf-8") as f:
        f.write(text)
    with open(os.path.join(out, f"{name}.logcat.txt"), "w", encoding="utf-8") as f:
        f.write(logcat)
    found = re.search(r"^OK \((\d+) tests?\)", text, re.M)
    bad = next((mark for mark in BAD if mark in text), None)
    if found and int(found.group(1)) == wanted and bad is None:
        print(f"PASS {name}: {wanted} tests, {took:.0f}s")
        return True
    counted = found.group(0) if found else "no OK line"
    print(f"FAIL {name}: wanted {wanted} tests, got {counted}{f', {bad}' if bad else ''}, {took:.0f}s")
    for line in text.splitlines():
        if re.match(r"^\d+\) ", line):
            print(f"    {line}")
    return False


def death_cycle(seed, out):
    """One kill: arm, wait for it to be editing, force-stop at a random moment, and verify in a new process."""
    stop()
    adb("logcat", "-c")
    arm = subprocess.Popen([ADB, "-s", SERIAL, *instrument("-e", "death", "arm", "-e", "seed", str(seed), "-e", "class", f"{APP}.EngineEditDeathCheck#arm")],
                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
    until = time.time() + 120
    while f"armed {seed}" not in adb("logcat", "-d", "-s", "EngineEditDeath:I").stdout:
        if time.time() > until or arm.poll() is not None:
            arm.kill()
            print(f"FAIL death {seed}: never armed")
            return False
        time.sleep(0.2)
    time.sleep(random.Random(seed).uniform(0.5, 5))
    # It has to be editing when the kill comes. Ended already, it failed or crashed by itself, and
    # verify would pass on a budget that no kill ever touched.
    ended = arm.poll() is not None
    stop()
    text, _ = arm.communicate(timeout=60)
    logcat = adb("logcat", "-d").stdout
    with open(os.path.join(out, f"death-{seed}-arm.txt"), "w", encoding="utf-8") as f:
        f.write(text)
    with open(os.path.join(out, f"death-{seed}-arm.logcat.txt"), "w", encoding="utf-8") as f:
        f.write(logcat)
    why = ("it ended before the kill" if ended else
           "it finished by itself" if re.search(r"^OK \(", text, re.M) else
           "it failed" if "FAILURES!!!" in text or "INSTRUMENTATION_FAILED" in text else
           "it crashed" if CRASH.search(logcat) else None)
    if why:
        print(f"FAIL death {seed}: the arm phase proved nothing, {why}")
        return False
    adb("logcat", "-c")
    done = adb(*instrument("-e", "death", "verify", "-e", "class", f"{APP}.EngineEditDeathCheck#verify"))
    verify = done.stdout + done.stderr
    with open(os.path.join(out, f"death-{seed}-verify.txt"), "w", encoding="utf-8") as f:
        f.write(verify)
    found = re.search(r"verified at step .*", adb("logcat", "-d", "-s", "EngineEditDeath:I").stdout)
    with open(os.path.join(out, f"death-{seed}-verify.logcat.txt"), "w", encoding="utf-8") as f:
        f.write(adb("logcat", "-d").stdout)
    if not re.search(r"^OK \(1 test\)", verify, re.M) or any(mark in verify for mark in BAD):
        print(f"FAIL death {seed}")
        print(verify[-1500:])
        return False
    print(f"PASS death {seed}: {found.group(0) if found else 'verified'}")
    return True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--only", nargs="*", help="classes to run, as named in CLASSES")
    parser.add_argument("--deaths", type=int, default=10)
    parser.add_argument("--no-deaths", action="store_true")
    parser.add_argument("--out", default=None)
    args = parser.parse_args()
    if SERIAL not in subprocess.run([ADB, "devices"], capture_output=True, text=True).stdout:
        sys.exit(f"{SERIAL} isn't running")
    out = args.out or os.path.join(os.path.dirname(os.path.abspath(__file__)), "edit-checks-" + datetime.datetime.now().strftime("%Y%m%d-%H%M%S"))
    os.makedirs(out, exist_ok=True)
    print(f"Logs in {out}")
    results = [run_class(name, CLASSES[name], out) for name in (args.only if args.only is not None else CLASSES)]
    if not args.no_deaths:
        results += [death_cycle(seed, out) for seed in range(1, args.deaths + 1)]
    passed = sum(results)
    print(f"{passed} of {len(results)} passed")
    sys.exit(0 if passed == len(results) else 1)


if __name__ == "__main__":
    main()
