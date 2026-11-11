import sys
import unittest
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from locators import UI
from safety import ReviewError
from walkthrough import Walkthrough


class LocatorTests(unittest.TestCase):
    def test_new_and_cleared_transaction_rows_have_different_element_types(self):
        for kind in ("XCUIElementTypeOther", "XCUIElementTypeStaticText"):
            xml = f'''<App><{kind} visible="true" accessible="true"
                label="Review Cafe, -12.50 SAR, Groceries, Checking, App Review demonstration, Uncleared">
                <XCUIElementTypeStaticText visible="true" accessible="false" label="Review Cafe"/>
                </{kind}><XCUIElementTypeOther visible="false" accessible="true"
                label="Review Cafe, -12.50 SAR, App Review demonstration"/></App>'''
            ui = UI(SimpleNamespace(page_source=xml))
            ui.tab = lambda _: None
            walkthrough = Walkthrough.__new__(Walkthrough)
            walkthrough.ui = ui
            with self.subTest(kind=kind):
                self.assertEqual(walkthrough.transaction_count(), 1)

    def test_ambiguous_locator_stops_without_guessing(self):
        ui = UI(SimpleNamespace(find_elements=lambda *args: [object(), object()]), timeout=.1)
        with self.assertRaises(ReviewError):
            ui.button("Accounts")

    def test_sync_status_belongs_to_selected_sample(self):
        xml = '''<App><Row><XCUIElementTypeStaticText label="Other budget" visible="true"/>
          <XCUIElementTypeStaticText label="Synced" visible="true"/></Row>
          <Row><XCUIElementTypeStaticText label="Sample" visible="true"/>
          <XCUIElementTypeStaticText label="This device only" visible="true"/></Row></App>'''
        ui = UI(SimpleNamespace(page_source=xml))
        ui.button = lambda _: None
        self.assertEqual(ui.sync_state("Sample"), "This device only")
        with self.assertRaises(ReviewError):
            ui.sync_state("Absent")


if __name__ == "__main__":
    unittest.main()
