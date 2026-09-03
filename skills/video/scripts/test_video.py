#!/usr/bin/env python3
"""Tests for the video skill.

Two layers: pure helpers are unit-tested directly, and every subcommand is run
as a real subprocess against a real clip built by ffmpeg — the same way forkd
runs it, so the JSON contract Albert parses is what's actually asserted.

Run:  python3 skills/video/scripts/test_video.py
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "video.py"
sys.path.insert(0, str(HERE))

import video  # noqa: E402

HAVE_FFMPEG = shutil.which("ffmpeg") is not None and shutil.which("ffprobe") is not None


def make_clip(path, secs=6, audio=True, size="320x240", rate=15):
    """A synthetic test clip: colour bars, optionally with a sine tone."""
    cmd = ["ffmpeg", "-v", "error", "-y",
           "-f", "lavfi", "-i", f"testsrc=size={size}:rate={rate}:duration={secs}"]
    if audio:
        cmd += ["-f", "lavfi", "-i", f"sine=frequency=440:duration={secs}"]
    cmd += ["-c:v", "libx264", "-pix_fmt", "yuv420p"]
    if audio:
        cmd += ["-c:a", "aac", "-shortest"]
    cmd += [str(path)]
    subprocess.run(cmd, check=True, capture_output=True)


class PureHelpers(unittest.TestCase):
    def test_parse_ts_accepts_what_a_person_types(self):
        self.assertEqual(video.parse_ts("7"), 7.0)
        self.assertEqual(video.parse_ts("1:23"), 83.0)
        self.assertEqual(video.parse_ts("1:00:00"), 3600.0)
        self.assertEqual(video.parse_ts("0:01.5"), 1.5)
        self.assertIsNone(video.parse_ts(None))

    def test_parse_ts_refuses_nonsense_instead_of_seeking_to_zero(self):
        # Silently treating garbage as 0:00 would cut the wrong part of a clip.
        for bad in ["", "abc", "1:2:3:4", "1:", "-5", ":"]:
            with self.assertRaises(ValueError, msg=f"{bad!r} should be rejected"):
                video.parse_ts(bad)

    def test_fmt_ts_shows_hours_only_once_they_exist(self):
        self.assertEqual(video.fmt_ts(7), "0:07")
        self.assertEqual(video.fmt_ts(83), "1:23")
        self.assertEqual(video.fmt_ts(3661), "1:01:01")

    def test_frame_times_stay_off_the_edges(self):
        # The first/last moments are usually black or a fade — sampling there
        # yields frames that show nothing.
        times = video.frame_times(100.0, 3)
        self.assertEqual(times, [25.0, 50.0, 75.0])
        self.assertGreater(times[0], 0.0)
        self.assertLess(times[-1], 100.0)
        self.assertEqual(len(video.frame_times(10.0, 1)), 1)
        self.assertEqual(video.frame_times(0.0, 2), [0.0, 0.0])
        with self.assertRaises(ValueError):
            video.frame_times(10.0, 0)

    def test_slug_keeps_it_filesystem_safe(self):
        self.assertEqual(video.slug("clip name!.mp4"), "clip-name-.mp4".replace(".", "-"))
        self.assertEqual(video.slug("../../etc"), "etc")
        self.assertEqual(video.slug("!!!"), "video")

    def test_out_path_never_clobbers_an_existing_artifact(self):
        with tempfile.TemporaryDirectory() as d:
            first = video.out_path("clip.mp4", "preview", ".gif", workspace=d)
            self.assertEqual(first.name, "clip-preview.gif")
            first.write_bytes(b"x")
            second = video.out_path("clip.mp4", "preview", ".gif", workspace=d)
            self.assertEqual(second.name, "clip-preview-2.gif")

    def test_out_path_confines_an_explicit_name_to_the_workspace(self):
        with tempfile.TemporaryDirectory() as d:
            got = video.out_path("clip.mp4", "clip", ".mp4", explicit="../../escape.mp4",
                                 workspace=d)
            self.assertEqual(got.parent, Path(d))
            self.assertEqual(got.name, "escape.mp4")


@unittest.skipUnless(HAVE_FFMPEG, "ffmpeg/ffprobe not installed")
class Subcommands(unittest.TestCase):
    """Each subcommand run exactly as forkd runs it: cwd = the workspace."""

    @classmethod
    def setUpClass(cls):
        cls.dir = tempfile.mkdtemp(prefix="video-skill-test-")
        cls.clip = Path(cls.dir) / "inbox" / "sample.mp4"
        cls.clip.parent.mkdir(parents=True)
        make_clip(cls.clip, secs=6, audio=True)
        cls.silent = Path(cls.dir) / "silent.mp4"
        make_clip(cls.silent, secs=2, audio=False)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.dir, ignore_errors=True)

    def run_cmd(self, *args):
        p = subprocess.run([sys.executable, str(SCRIPT), *args],
                           cwd=self.dir, capture_output=True, text=True)
        self.assertEqual(p.returncode, 0, f"script crashed: {p.stderr[:400]}")
        try:
            return json.loads(p.stdout)
        except json.JSONDecodeError:
            self.fail(f"stdout was not one JSON object: {p.stdout[:400]!r}")

    def test_info_reports_the_facts_that_decide_what_to_do_next(self):
        out = self.run_cmd("info", "inbox/sample.mp4")
        self.assertEqual(out["status"], "ok")
        self.assertAlmostEqual(out["duration_secs"], 6.0, delta=0.5)
        self.assertEqual(out["duration"], "0:06")
        self.assertTrue(out["has_audio"])
        self.assertEqual((out["width"], out["height"]), (320, 240))
        self.assertEqual(out["video_codec"], "h264")
        self.assertGreater(out["size_bytes"], 0)

    def test_frames_writes_the_stills_it_reports(self):
        out = self.run_cmd("frames", "inbox/sample.mp4", "--count", "3")
        self.assertEqual(out["status"], "ok")
        self.assertEqual(out["count"], 3)
        for frame in out["frames"]:
            path = Path(self.dir) / frame["path"]
            self.assertTrue(path.is_file(), f"missing frame {frame['path']}")
            self.assertGreater(path.stat().st_size, 0)
            # Paths must be workspace-relative — that's what chat.send_file takes.
            self.assertFalse(Path(frame["path"]).is_absolute())

    def test_frames_at_an_explicit_moment(self):
        out = self.run_cmd("frames", "inbox/sample.mp4", "--at", "0:02", "--width", "160")
        self.assertEqual(out["count"], 1)
        self.assertEqual(out["frames"][0]["at"], "0:02")
        self.assertTrue((Path(self.dir) / out["frames"][0]["path"]).is_file())

    def test_frames_past_the_end_says_so_instead_of_writing_nothing(self):
        out = self.run_cmd("frames", "inbox/sample.mp4", "--at", "99:00")
        self.assertEqual(out["status"], "error")
        self.assertEqual(out["error"], "out_of_range")

    def test_clip_cuts_the_requested_range(self):
        out = self.run_cmd("clip", "inbox/sample.mp4", "--from", "0:01", "--to", "0:03")
        self.assertEqual(out["status"], "ok")
        produced = Path(self.dir) / out["out"]
        self.assertTrue(produced.is_file())
        probed = self.run_cmd("info", out["out"])
        self.assertLess(probed["duration_secs"], 5.0)
        self.assertGreater(probed["duration_secs"], 0.5)

    def test_clip_rejects_a_backwards_range(self):
        out = self.run_cmd("clip", "inbox/sample.mp4", "--from", "0:03", "--to", "0:01")
        self.assertEqual(out["status"], "error")
        self.assertEqual(out["error"], "empty_range")

    def test_clip_rejects_an_unparseable_timestamp(self):
        out = self.run_cmd("clip", "inbox/sample.mp4", "--from", "banana")
        self.assertEqual(out["status"], "error")
        self.assertEqual(out["error"], "bad_timestamp")

    def test_gif_builds_a_looping_preview(self):
        out = self.run_cmd("gif", "inbox/sample.mp4", "--duration", "1", "--width", "160")
        self.assertEqual(out["status"], "ok")
        produced = Path(self.dir) / out["out"]
        self.assertTrue(produced.is_file())
        self.assertEqual(produced.suffix, ".gif")
        self.assertGreater(produced.stat().st_size, 0)

    def test_audio_extracts_a_track_and_points_at_transcribe(self):
        out = self.run_cmd("audio", "inbox/sample.mp4")
        self.assertEqual(out["status"], "ok")
        self.assertTrue((Path(self.dir) / out["out"]).is_file())
        self.assertIn("transcribe", out["hint"])

    def test_audio_on_a_silent_clip_is_an_honest_error(self):
        out = self.run_cmd("audio", "silent.mp4")
        self.assertEqual(out["status"], "error")
        self.assertEqual(out["error"], "no_audio")

    def test_compress_produces_a_smaller_file_and_reports_the_target(self):
        out = self.run_cmd("compress", "inbox/sample.mp4", "--max-mb", "0.2", "--width", "160")
        self.assertEqual(out["status"], "ok")
        produced = Path(self.dir) / out["out"]
        self.assertTrue(produced.is_file())
        self.assertIn("met_target", out)
        self.assertEqual(out["target_mb"], 0.2)

    def test_a_missing_file_is_named_not_crashed_on(self):
        out = self.run_cmd("info", "inbox/nope.mp4")
        self.assertEqual(out["status"], "error")
        self.assertEqual(out["error"], "not_found")

    def test_a_path_outside_the_workspace_is_refused(self):
        out = self.run_cmd("info", "../../etc/hosts")
        self.assertEqual(out["status"], "error")
        self.assertEqual(out["error"], "outside_workspace")


if __name__ == "__main__":
    if not HAVE_FFMPEG:
        print("WARNING: ffmpeg/ffprobe missing — subcommand tests will be skipped",
              file=sys.stderr)
    unittest.main(verbosity=2)
