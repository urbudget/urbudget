"""iPhone Control Center recording, observed on iOS 27.2; never Appium video capture."""
from __future__ import annotations

from selenium.webdriver.support.ui import WebDriverWait

from safety import ReviewError, now


class NativeRecording:
    def __init__(self, ui, journal, manual=False):
        self.ui, self.journal, self.manual = ui, journal, manual

    def home(self):
        # WDA kept the old UrBudget snapshot after Home. Explicitly target SpringBoard.
        self.ui.activate("com.apple.springboard")
        self.ui.driver.execute_script("mobile: pressButton", {"name": "home"})
        self.ui.active("com.apple.springboard")

    def control_center(self):
        self.home()
        size = self.ui.driver.get_window_size()
        if size["height"] <= size["width"]:
            raise ReviewError("Use the verified portrait orientation for Control Center.")
        self.ui.driver.execute_script("mobile: dragFromToForDuration", {
            "duration": 0.3, "fromX": size["width"] * .95, "fromY": 1,
            "toX": size["width"] * .95, "toY": size["height"] * .19})
        # This phone has Favorites / Now Playing / Connectivity. A deep opening
        # swipe can land on page 3. Never substitute an unobserved coordinate tap.
        for _ in range(3):
            if self.ui.shown("Screen Recording"):
                return
            self.ui.scroll("down")
        raise ReviewError("Native recording control unavailable; use the manual checkpoint mode.")

    def start(self, handoff):
        if self.journal.data.get("recording", {}).get("start_intent_at") or self.journal.data.get("recording", {}).get("started_at"):
            raise ReviewError("This run already contains a capture attempt; do not overwrite its provenance.")
        self.home()
        if self.manual:
            handoff("On the phone start built-in Screen Recording, wait for its countdown, then return Home. Confirm the red recording indicator.")
        else:
            self.control_center()
            if self.ui.button("Screen Mirroring").get_attribute("value") != "Off":
                raise ReviewError("Close screen mirroring before native recording.")
            if self.ui.button("Screen Recording").get_attribute("value") == "Recording":
                raise ReviewError("A recording is already running; stop and retain it first.")
            # Persist before tapping: never silently repeat a start after an uncertain response.
            self.journal.data["recording"] = {"method": "iPhone built-in Screen Recording", "start_intent_at": now()}
            self.journal.save()
            self.ui.tap("Screen Recording")
            WebDriverWait(self.ui.driver, 10).until(
                lambda _: self.ui.button("Screen Recording").get_attribute("value") == "Recording")
            self.home()
        self.journal.data.setdefault("recording", {}).update({"method": "iPhone built-in Screen Recording",
            "controls": "manual" if self.manual else "observed Control Center UI", "started_at": now(), "photos_save_confirmed": False})
        self.journal.save()

    def stop(self, handoff, *, confirm_photos=True):
        record = self.journal.data.get("recording", {})
        if not record.get("started_at") or record.get("stopped_at"):
            raise ReviewError("No running capture belongs to this run.")
        if self.manual:
            handoff("Stop native Screen Recording on the phone.")
        else:
            self.control_center()
            if self.ui.button("Screen Recording").get_attribute("value") != "Recording":
                raise ReviewError("Recording stopped outside the harness; inspect Photos and retain the actual outcome.")
            self.ui.tap("Screen Recording")
            WebDriverWait(self.ui.driver, 10).until(
                lambda _: self.ui.button("Screen Recording").get_attribute("value") != "Recording")
            self.home()
        record["stopped_at"] = now()
        self.journal.save()
        if confirm_photos:
            handoff("Verify that Photos saved this new recording and that it plays. Keep the original on the phone.")
            record["photos_save_confirmed"] = True
            self.journal.save()
