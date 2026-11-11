import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path
from subprocess import CompletedProcess
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from safety import ReviewError
from verify_video import inspect


class VideoTests(unittest.TestCase):
    def test_full_decode_failure_is_saved_and_never_ready(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            video = root / "reports" / "original.MP4"
            video.parent.mkdir()
            video.write_bytes(b"fixture only")
            output = video.parent / "verification"
            metadata = {"format": {"duration": "2.5"}, "streams": [
                {"codec_type": "video", "width": 1320, "height": 2868, "codec_name": "hevc"}]}
            results = [CompletedProcess([], 0, json.dumps(metadata).encode(), b""),
                       CompletedProcess([], 1, b"", b"corrupt frame")]
            with patch("verify_video.__file__", str(root / "ios/automation/app-review/verify_video.py")), \
                 patch("verify_video.subprocess.run", side_effect=results) as run:
                with self.assertRaises(ReviewError):
                    inspect(video, {}, output, playback_reviewed=True, original_confirmed=True)
            self.assertIn("-xerror", run.call_args_list[1].args[0])
            self.assertNotIn("-t", run.call_args_list[1].args[0])  # Decode the whole original.
            manifest = json.loads((output / "video-manifest.json").read_text())
            self.assertFalse(manifest["full_decode_passed"])
            self.assertFalse(manifest["ready_for_review"])
            self.assertEqual(manifest["sha256"], hashlib.sha256(video.read_bytes()).hexdigest())
            self.assertEqual((output / "decode.log").stat().st_mode & 0o777, 0o600)

    def test_successful_decode_does_not_supply_missing_coverage(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            video = root / "reports" / "probe.MP4"
            video.parent.mkdir()
            video.write_bytes(b"fixture only")
            metadata = {"format": {"duration": "2.5"}, "streams": [{"codec_type": "video", "width": 100}]}
            with patch("verify_video.__file__", str(root / "ios/automation/app-review/verify_video.py")), \
                 patch("verify_video.subprocess.run", side_effect=[
                     CompletedProcess([], 0, json.dumps(metadata).encode(), b""), CompletedProcess([], 0, b"", b"")]):
                manifest = inspect(video, {}, video.parent / "qa", playback_reviewed=True, original_confirmed=True)
            self.assertTrue(manifest["full_decode_passed"])
            self.assertFalse(manifest["ready_for_review"])
            self.assertEqual(len(manifest["missing_stages"]), 9)


if __name__ == "__main__":
    unittest.main()
