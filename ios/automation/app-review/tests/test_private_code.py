import copy
import sys
import unittest
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from private_code import validate_receipt
from safety import ReviewError


class PrivateCodeTests(unittest.TestCase):
    def test_receipt_requires_matching_identity_and_freshness(self):
        receipt = {"email": "sample@example.invalid", "code": "123456",
                   "received_at": "2026-10-31T09:01:00+00:00",
                   "expires_at": "2026-10-31T09:06:00+00:00"}
        current = datetime(2026, 10, 31, 9, 2, tzinfo=timezone.utc)
        requested = "2026-10-31T09:00:00+00:00"
        self.assertEqual(validate_receipt(receipt, receipt["email"], requested, current), "123456")
        for key, value in (("email", "another@example.invalid"),
                           ("received_at", "2026-10-31T08:59:00+00:00"),
                           ("expires_at", "2026-10-31T09:01:30+00:00"),
                           ("expires_at", "2026-10-31T10:01:00+00:00"),
                           ("code", "１２３４５６"), ("code", "12345")):
            bad = copy.deepcopy(receipt)
            bad[key] = value
            with self.subTest(key=key), self.assertRaises(ReviewError):
                validate_receipt(bad, receipt["email"], requested, current)


if __name__ == "__main__":
    unittest.main()
