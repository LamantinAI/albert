#!/usr/bin/env python3
"""Tests for the transcribe skill's media handling.

Nothing here touches the network or the token store — only the ffmpeg side,
which is where a video input used to go pathological.

Run:  python3 skills/transcribe/scripts/test_transcribe.py
"""
import json
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import transcribe  # noqa: E402

HAVE_FFMPEG = shutil.which("ffmpeg") is not None and shutil.which("ffprobe") is not None


def streams(path):
    """Codec types present in a file, per ffprobe."""
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-print_format", "json", "-show_streams", str(path)],
        capture_output=True, text=True, check=True).stdout
    return [s["codec_type"] for s in json.loads(out)["streams"]]


@unittest.skipUnless(HAVE_FFMPEG, "ffmpeg/ffprobe not installed")
class Cut(unittest.TestCase):
    """`cut` must produce audio, and only audio, whatever it is handed."""

    @classmethod
    def setUpClass(cls):
        cls.dir = tempfile.mkdtemp(prefix="transcribe-test-")
        cls.video = Path(cls.dir) / "clip.mp4"
        subprocess.run(
            ["ffmpeg", "-v", "error", "-y",
             "-f", "lavfi", "-i", "testsrc=size=640x480:rate=25:duration=8",
             "-f", "lavfi", "-i", "sine=frequency=440:duration=8",
             "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest",
             str(cls.video)], check=True, capture_output=True)
        cls.audio = Path(cls.dir) / "voice.m4a"
        subprocess.run(
            ["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=8",
             "-c:a", "aac", str(cls.audio)], check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.dir, ignore_errors=True)

    def test_a_video_chunk_carries_no_video_stream(self):
        # The regression this guards: without -vn the .webm output also gets the
        # video stream re-encoded to VP9 — minutes of CPU and a huge file, for
        # pixels the dictation endpoint never reads.
        out = transcribe.cut(str(self.video), 0.0, 8.0, self.dir, 0)
        self.assertEqual(streams(out), ["audio"],
                         "a chunk must be audio-only — a video stream means -vn was lost")

    def test_a_video_chunk_stays_the_size_of_speech_not_of_pixels(self):
        out = transcribe.cut(str(self.video), 0.0, 8.0, self.dir, 1)
        size = Path(out).stat().st_size
        # 8 s of 24 kbps Opus is ~24 KB; anything near a megabyte is video.
        self.assertLess(size, 200_000, f"chunk is {size} bytes — video is being packed in")

    def test_cutting_a_video_is_fast(self):
        # VP9 re-encoding took minutes for a couple of minutes of input; audio-only
        # is near-instant. A generous ceiling still catches the pathology.
        start = time.time()
        transcribe.cut(str(self.video), 0.0, 8.0, self.dir, 2)
        self.assertLess(time.time() - start, 20.0, "cutting audio should not take this long")

    def test_an_audio_only_input_still_works(self):
        # The path that always worked must keep working.
        out = transcribe.cut(str(self.audio), 0.0, 8.0, self.dir, 3)
        self.assertEqual(streams(out), ["audio"])
        self.assertGreater(Path(out).stat().st_size, 0)

    def test_the_requested_range_is_what_gets_cut(self):
        out = transcribe.cut(str(self.video), 2.0, 5.0, self.dir, 4)
        dur = float(subprocess.run(
            ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", out],
            capture_output=True, text=True, check=True).stdout.strip())
        self.assertAlmostEqual(dur, 3.0, delta=0.6)


if __name__ == "__main__":
    unittest.main(verbosity=2)
