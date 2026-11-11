"""English TestFlight build 1 selectors; stop on absent or ambiguous controls."""
from __future__ import annotations

import json
import time
import xml.etree.ElementTree as ET

from appium.webdriver.common.appiumby import AppiumBy
from selenium.common.exceptions import StaleElementReferenceException
from selenium.webdriver.support.ui import WebDriverWait

from safety import ReviewError


class UI:
    def __init__(self, driver, timeout=20, pause=2):
        self.driver, self.timeout, self.pause = driver, timeout, pause

    def one(self, predicate, scope=None):
        def find(_):
            try:
                matches = (scope or self.driver).find_elements(AppiumBy.IOS_PREDICATE,
                    f'({predicate}) AND visible == true AND enabled == true')
                if len(matches) > 1:
                    raise ReviewError("Ambiguous visible locator; inspect the sample-data screen.")
                return matches[0] if matches else False
            except StaleElementReferenceException:
                return False  # A read can retry; a click/save never does.
        return WebDriverWait(self.driver, self.timeout).until(find)

    def button(self, label):
        return self.one('type == "XCUIElementTypeButton" AND label == ' + json.dumps(label))

    def prefix(self, prefix, kind="Button"):
        return self.one(f'type == "XCUIElementType{kind}" AND label BEGINSWITH ' + json.dumps(prefix))

    def tap(self, label):
        self.button(label).click()

    def tab(self, label):
        # "Accounts" is also a back button in the account register.
        bar = self.one('type == "XCUIElementTypeTabBar" AND label == "Tab Bar"')
        self.one('type == "XCUIElementTypeButton" AND label == ' + json.dumps(label), bar).click()
        bar = self.one('type == "XCUIElementTypeTabBar" AND label == "Tab Bar"')
        self.one('type == "XCUIElementTypeButton" AND label == ' + json.dumps(label) + ' AND value == "1"', bar)

    def field(self, value, kind="TextField"):
        return self.one(f'type == "XCUIElementType{kind}" AND value == ' + json.dumps(value))

    def finish_email_entry(self):
        # iOS Hide My Email can cover the request button while this field is
        # focused. The observed email keyboard's Return dismisses it without
        # requesting a code; verify dismissal before resolving the button again.
        if self.driver.is_keyboard_shown():
            self.tap("return")
            WebDriverWait(self.driver, self.timeout).until(lambda _: not self.driver.is_keyboard_shown())

    def more_menu(self):
        self.tab("More")
        # A tab retains its navigation stack. Pop only the observed back control,
        # never the identically labelled More tab or a guessed coordinate.
        predicate = 'type == "XCUIElementTypeButton" AND label BEGINSWITH "Account & sync," AND visible == true'
        for _ in range(4):
            if self.driver.find_elements(AppiumBy.IOS_PREDICATE, predicate):
                return
            self.one('type == "XCUIElementTypeButton" AND name == "BackButton"').click()
        raise ReviewError("Could not return to the More menu.")

    def reports_menu(self):
        self.tab("Reports")
        for _ in range(4):
            if self.shown("Spending breakdown"):
                return
            self.one('type == "XCUIElementTypeButton" AND name == "BackButton"').click()
        raise ReviewError("Could not return to the report overview.")

    def text(self, label):
        return self.one('type == "XCUIElementTypeStaticText" AND label == ' + json.dumps(label))

    def shown(self, label, kind="XCUIElementTypeButton"):
        return self.driver.find_elements(AppiumBy.IOS_PREDICATE,
                'type == ' + json.dumps(kind) + ' AND label == ' + json.dumps(label) + ' AND visible == true')

    def tree(self):
        # Only call on sample-data screens; never around credentials/provider settings.
        return ET.fromstring(self.driver.page_source)

    def visible(self):
        return [e for e in self.tree().iter() if e.get("visible") == "true" and e.get("accessible") == "true"]

    def sync_state(self, budget_name):
        self.button("Sync now")  # No passphrase editor may be open during inspection.
        tree = self.tree()
        parents = {child: parent for parent in tree.iter() for child in parent}
        names = [e for e in tree.iter() if e.tag == "XCUIElementTypeStaticText"
                 and e.get("label") == budget_name and e.get("visible") == "true"]
        if len(names) != 1:
            raise ReviewError("The selected sample's Sync row must be uniquely visible.")
        states = {"Synced", "Uploaded", "Downloaded", "Merged", "Error", "Needs review",
                  "This device only", "Waiting", "Update the app", "In the cloud"}
        node = names[0]
        for _ in range(8):
            node = parents.get(node)
            if node is None:
                break
            found = [e.get("label") for e in node.iter() if e.tag == "XCUIElementTypeStaticText"
                     and e.get("visible") == "true" and e.get("label") in states]
            if len(found) == 1:
                return found[0]
            if len(found) > 1:
                break
        raise ReviewError("Could not associate a Sync status with this sample budget.")

    def start_sample_sync(self, budget_name):
        if self.sync_state(budget_name) != "This device only":
            raise ReviewError("The selected sample is not awaiting Sync enrollment.")
        if '"' in budget_name or "'" in budget_name:
            raise ReviewError("Use a simple sample name for the verified enrollment locator.")
        # Element-relative XPath cannot ascend beyond WDA's subtree snapshot.
        # Start from the root and use the observed name/status/action row instead.
        matches = self.driver.find_elements(AppiumBy.XPATH,
            f'//XCUIElementTypeStaticText[@label="{budget_name}" and @visible="true"]/../..'
            '//XCUIElementTypeButton[@label="Start syncing" and @visible="true"]')
        if len(matches) != 1:
            raise ReviewError("Could not uniquely scope enrollment to the selected sample.")
        matches[0].click()

    def active(self, bundle):
        WebDriverWait(self.driver, self.timeout).until(
            lambda d: d.execute_script("mobile: activeAppInfo").get("bundleId") == bundle)

    def activate(self, bundle):
        self.driver.activate_app(bundle)
        self.active(bundle)

    def result_pause(self):
        time.sleep(self.pause)  # Readability pause only, after a checked postcondition.

    def digits(self, amount):
        for digit in str(amount):
            self.tap(digit)

    def scroll(self, direction):
        self.driver.execute_script("mobile: swipe", {"direction": direction})

    def reveal(self, label, *, attempts=4):
        for _ in range(attempts):
            matches = self.shown(label)
            if len(matches) > 1:
                raise ReviewError("Ambiguous scroll destination.")
            if matches:
                return matches[0]
            self.scroll("up")
        raise ReviewError("Expected control was not reached by bounded scrolling.")
