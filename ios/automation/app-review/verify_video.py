"""Hash, probe and fully decode an exported original. Coverage and playback remain explicit."""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from datetime import datetime
from pathlib import Path

from safety import ReviewError, now, private_json

REQUIRED = ("launch", "assign", "outflow", "reports", "review_access", "sync", "assistant", "registration", "deletion")


def coverage(journal, *, playback_reviewed=False, original_confirmed=False):
    stages = journal.get("stages", {})
    missing = [name for name in REQUIRED if stages.get(name, {}).get("status") != "passed"]
    capture = journal.get("recording", {})
    identity = journal.get("identity", {})
    reasons = []
    if journal.get("mode") != "final":
        reasons.append("This journal is not a final take")
    if missing:
        reasons.append("Unverified walkthrough stages: " + ", ".join(missing))
    if capture.get("method") != "iPhone built-in Screen Recording" or not capture.get("stopped_at") or not capture.get("photos_save_confirmed"):
        reasons.append("Native capture and Photos save are not fully confirmed")
    if identity.get("distribution") != "TestFlight build 1 Open button launched app.urbudget":
        reasons.append("TestFlight identity has not been verified")
    if (identity.get("bundle"), identity.get("version"), identity.get("build")) != ("app.urbudget", "1.0.0", "1") or not identity.get("os"):
        reasons.append("Capture device/app identity is incomplete")
    # Passed rehearsal stages must not certify unrelated or later footage.
    try:
        start, stop = (datetime.fromisoformat(capture[key]) for key in ("started_at", "stopped_at"))
        if start.tzinfo is None or stop.tzinfo is None or stop <= start:
            raise ValueError
        last = start
        for name in REQUIRED:
            stage_start = datetime.fromisoformat(stages[name]["started_at"])
            stage_stop = datetime.fromisoformat(stages[name]["at"])
            if not last <= stage_start <= stage_stop <= stop:
                raise ValueError
            last = stage_stop
    except (KeyError, TypeError, ValueError):
        reasons.append("All stages must occur in order within this native capture")
    if not original_confirmed:
        reasons.append("The selected file has not been matched to this take's Photos original")
    if not playback_reviewed:
        reasons.append("Full human playback/privacy review is still required")
    return {"stages": stages, "missing_stages": missing, "limitations": reasons,
            "coverage_complete": not reasons, "human_playback_reviewed": playback_reviewed,
            "original_matched": original_confirmed}


def inspect(video: Path, journal, destination: Path, *, playback_reviewed=False, original_confirmed=False):
    video, destination = video.resolve(), destination.resolve()
    reports = Path(__file__).resolve().parents[3] / "reports"
    if not video.is_relative_to(reports) or not destination.is_relative_to(reports):
        raise ReviewError("Keep the original and private verification artifacts under ignored reports.")
    if not video.is_file() or video.stat().st_size == 0:
        raise ReviewError("Select the exported original video file.")
    with video.open("rb") as original:
        digest = hashlib.file_digest(original, "sha256").hexdigest()
    probe = subprocess.run(["ffprobe", "-v", "error", "-show_entries",
        "format=duration,size:stream=index,codec_name,codec_type,width,height,avg_frame_rate,r_frame_rate",
        "-of", "json", str(video)], capture_output=True, timeout=60)
    if probe.returncode:
        raise ReviewError("ffprobe could not read the selected original.")
    metadata = json.loads(probe.stdout)
    if not any(s.get("codec_type") == "video" and s.get("width", 0) > 0 for s in metadata.get("streams", [])):
        raise ReviewError("The selected file has no decodable video stream.")
    if float(metadata.get("format", {}).get("duration", 0)) <= 0:
        raise ReviewError("The video has no positive duration.")
    destination.mkdir(parents=True, exist_ok=True, mode=0o700)
    decode = subprocess.run(["ffmpeg", "-v", "error", "-xerror", "-i", str(video), "-f", "null", "-"],
                            capture_output=True, timeout=1800)
    (destination / "decode.log").write_bytes(decode.stderr)
    (destination / "decode.log").chmod(0o600)
    manifest = {"verified_at": now(), "original_file": str(video), "sha256": digest,
                "probe": metadata, "full_decode_passed": decode.returncode == 0,
                "execution_mode": journal.get("mode"), "deviations": journal.get("deviations", []),
                "old_session_revoked": journal.get("old_session_revoked", False),
                "identity": journal.get("identity", {}), "recording": journal.get("recording", {}),
                **coverage(journal, playback_reviewed=playback_reviewed, original_confirmed=original_confirmed)}
    manifest["ready_for_review"] = manifest["full_decode_passed"] and manifest["coverage_complete"]
    private_json(destination / "video-manifest.json", manifest)
    if decode.returncode:
        raise ReviewError("Full video decode failed; retain the original and decode log.")
    return manifest


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("video", type=Path)
    p.add_argument("--journal", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--playback-reviewed", action="store_true", help="Only after watching the entire original for content, legibility, smoothness and secrets")
    p.add_argument("--original-confirmed", action="store_true", help="Only after matching this file to this take's unmodified Photos original by capture time and contents")
    a = p.parse_args()
    try:
        result = inspect(a.video, json.loads(a.journal.read_text()), a.output,
                         playback_reviewed=a.playback_reviewed, original_confirmed=a.original_confirmed)
        print(json.dumps({"sha256": result["sha256"], "ready_for_review": result["ready_for_review"], "limitations": result["limitations"]}, indent=2))
    except (ReviewError, subprocess.TimeoutExpired) as error:
        raise SystemExit(str(error) if isinstance(error, ReviewError) else "Video inspection timed out; coverage remains unverified.")
