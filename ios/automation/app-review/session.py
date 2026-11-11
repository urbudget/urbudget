"""Connect only to the selected physical iPhone's existing app; never build/install UrBudget."""
from __future__ import annotations

import argparse
import fcntl
import json
import re
import subprocess
import tempfile
from pathlib import Path

from safety import ReviewError, now, private_json

BUNDLE = "app.urbudget"
VERSION = "1.0.0"
BUILD = "1"
REPO = Path(__file__).resolve().parents[3]
DEFAULT_ROOT = REPO / "reports/app-review-mac"


def validate_identity(device, apps, udid):
    hardware, software = device.get("hardwareProperties", {}), device.get("deviceProperties", {})
    if hardware.get("udid") != udid or hardware.get("reality") != "physical" or hardware.get("platform") != "iOS":
        raise ReviewError("The selected identifier must resolve to the physical iPhone.")
    if not software.get("osVersionNumber") or software.get("developerModeStatus") != "enabled":
        raise ReviewError("Read the actual iOS version and enable Developer Mode first.")
    matches = [app for app in apps.get("apps", []) if app.get("bundleIdentifier") == BUNDLE]
    if len(matches) != 1 or (matches[0].get("version"), matches[0].get("bundleVersion")) != (VERSION, BUILD):
        raise ReviewError("The installed app must be app.urbudget 1.0.0 (1); do not replace it automatically.")
    return {"checked_at": now(), "udid": udid, "model": hardware.get("marketingName"),
            "os": software["osVersionNumber"], "os_build": software.get("osBuildUpdate"),
            "bundle": BUNDLE, "version": VERSION, "build": BUILD,
            "builtByDeveloper": matches[0].get("builtByDeveloper"),
            "distribution": "requires TestFlight UI verification"}


def capabilities(config):
    udid = config.get("udid", "")
    team, helper = config.get("team_id", ""), config.get("wda_bundle_id", "")
    if not re.fullmatch(r"[0-9A-Fa-f]{8}-[0-9A-Fa-f]{16}", udid):
        raise ReviewError("Set the explicitly selected physical-device UDID.")
    if not re.fullmatch(r"[A-Z0-9]{10}", team) or not re.fullmatch(r"app\.urbudget\.review\.[A-Za-z0-9.-]+", helper):
        raise ReviewError("Use the signing team and a separately provisioned review helper identifier.")
    caps = {"platformName": "iOS", "appium:automationName": "XCUITest", "appium:udid": udid,
            "appium:bundleId": BUNDLE, "appium:noReset": True, "appium:fullReset": False,
            "appium:autoLaunch": False, "appium:forceAppLaunch": False, "appium:shouldTerminateApp": False,
            "appium:useNewWDA": False, "appium:xcodeOrgId": team,
            "appium:xcodeSigningId": "Apple Development", "appium:updatedWDABundleId": helper,
            "appium:autoAcceptAlerts": False, "appium:autoDismissAlerts": False,
            "appium:printPageSourceOnFindFailure": False, "appium:skipLogCapture": True,
            "appium:newCommandTimeout": 1800, "appium:useNativeCachingStrategy": False,
            "appium:waitForIdleTimeout": 1}
    # A helper already started by xcodebuild can be attached over USB or the paired LAN.
    if config.get("wda_url"):
        from urllib.parse import urlparse
        from ipaddress import ip_address
        url = urlparse(config["wda_url"])
        try:
            private = url.hostname == "localhost" or ip_address(url.hostname).is_private
        except ValueError:
            private = False
        if url.scheme != "http" or not private or url.username or url.password or url.query:
            raise ReviewError("WDA must use a verified local helper endpoint without embedded credentials.")
        caps["appium:webDriverAgentUrl"] = config["wda_url"]
    else:
        caps["appium:usePrebuiltWDA"] = True
        caps["appium:derivedDataPath"] = str(DEFAULT_ROOT / "wda-build")
    return caps


def device_query(arguments, root):
    with tempfile.TemporaryDirectory(dir=root) as temp:
        output = Path(temp) / "device.json"
        result = subprocess.run(["xcrun", "devicectl", *arguments, "--timeout", "30", "--quiet",
                                 "--json-output", str(output)], capture_output=True, timeout=40)
        if result.returncode or not output.exists():
            raise ReviewError("Xcode could not read the selected phone; check pairing, unlock and USB.")
        return json.loads(output.read_text())["result"]


class Session:
    def __init__(self, config_path, evidence):
        self.config = json.loads(Path(config_path).read_text())
        self.caps = capabilities(self.config)
        self.evidence = Path(evidence).resolve()
        if not self.evidence.is_relative_to((REPO / "reports").resolve()):
            raise ReviewError("Private evidence must stay under the ignored reports directory.")
        self.evidence.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.driver = None
        self.lock = None

    def inspect_identity(self):
        udid = self.config["udid"]
        device = device_query(["device", "info", "details", "--device", udid], self.evidence)
        apps = device_query(["device", "info", "apps", "--device", udid, "--bundle-id", BUNDLE], self.evidence)
        self.identity = validate_identity(device, apps, udid)
        private_json(self.evidence / "identity.json", self.identity)
        return self.identity

    def __enter__(self):
        from appium import webdriver
        from appium.options.ios import XCUITestOptions
        from locators import UI
        DEFAULT_ROOT.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.lock = (DEFAULT_ROOT / "device.lock").open("a")
        try:
            fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.inspect_identity()
            self.caps["appium:platformVersion"] = self.identity["os"]
            self.driver = webdriver.Remote("http://127.0.0.1:4723", options=XCUITestOptions().load_capabilities(self.caps))
            self.ui = UI(self.driver)
            return self
        except BaseException:
            self.__exit__(None, None, None)
            raise

    def __exit__(self, *_):
        try:
            if self.driver:
                self.driver.quit()
        finally:
            if self.lock:
                self.lock.close()

    def verify_testflight(self):
        self.ui.activate("com.apple.TestFlight")
        # Observed on the UrBudget Previous Builds page; never press Install/Update.
        self.ui.one('type == "XCUIElementTypeOther" AND label BEGINSWITH "Build: 1 . 0 . 0 (1), Expires in "')
        self.ui.tap("Open")
        self.ui.active(BUNDLE)  # Wait for the actual destination, not the transient TestFlight app.
        self.identity["distribution"] = "TestFlight build 1 Open button launched app.urbudget"
        private_json(self.evidence / "identity.json", self.identity)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, default=DEFAULT_ROOT / "private.json")
    args = parser.parse_args()
    session = Session(args.config, DEFAULT_ROOT / "evidence/preflight")
    print(json.dumps(session.inspect_identity(), indent=2))
