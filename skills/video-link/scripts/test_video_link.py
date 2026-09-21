#!/usr/bin/env python3
"""Tests for the video-link skill.

Pure helpers are unit-tested directly. The subcommands run as a real subprocess —
exactly as forkd runs them, cwd = a scratch workspace — with a real yt-dlp pulling a
clip that ffmpeg builds on the spot and a local HTTP server hands out. No network,
no YouTube: yt-dlp's generic extractor takes a direct media link like any site.

The live test is opt-in (VIDEO_LINK_LIVE=1): it fetches a real YouTube Short, and is
the canary for "yt-dlp has gone stale, bump the pinned version".

Run:  python3 skills/video-link/scripts/test_video_link.py
Live: VIDEO_LINK_LIVE=1 python3 skills/video-link/scripts/test_video_link.py
"""
import functools
import http.server
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "video_link.py"
sys.path.insert(0, str(HERE))

import video_link as vl  # noqa: E402

HAVE_TOOLS = all(shutil.which(t) for t in ("ffmpeg", "ffprobe")) and vl.ytdlp_bin() is not None
LIVE = os.environ.get("VIDEO_LINK_LIVE") == "1"
LIVE_URL = "https://youtube.com/shorts/0ucDb72zfrg"


def streams(path):
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-print_format", "json", "-show_streams", str(path)],
        capture_output=True, text=True, check=True).stdout
    return sorted(s["codec_type"] for s in json.loads(out)["streams"])


class PureHelpers(unittest.TestCase):
    def test_errors_map_to_codes_the_model_can_act_on(self):
        cases = {
            "ERROR: [youtube] x: Private video. Sign in if you've been granted access": "private",
            "ERROR: Video unavailable. This video has been removed by the uploader": "unavailable",
            "ERROR: HTTP Error 404: Not Found": "unavailable",
            "ERROR: Unsupported URL: https://example.com/": "unsupported_url",
            "ERROR: unable to download video data: HTTP Error 403: Forbidden": "forbidden",
            "ERROR: The uploader has not made this video available in your country": "geo_blocked",
            "ERROR: This video is not available in your country": "geo_blocked",
            "ERROR: Video unavailable. The uploader has blocked it in your country": "geo_blocked",
            "ERROR: Requested format is not available": "no_formats",
            "ERROR: Sign in to confirm you’re not a bot": "bot_check",
            "something nobody anticipated": "download_failed",
        }
        for stderr, code in cases.items():
            self.assertEqual(vl.classify_error(stderr), code, stderr)

    def test_the_age_gate_is_not_mistaken_for_a_bot_check(self):
        # Both say "sign in to confirm"; conflating them sends the reader after the
        # wrong fix (an IP ban vs. an account requirement).
        self.assertEqual(
            vl.classify_error("ERROR: Sign in to confirm your age. This video may be "
                              "inappropriate for some users."), "age_restricted")

    def test_every_error_code_has_a_hint(self):
        codes = {code for code, _ in vl.ERROR_PATTERNS} | {"download_failed"}
        self.assertEqual(codes - set(vl.HINTS), set())

    def test_duration_reads_like_a_person_says_it(self):
        self.assertEqual(vl.fmt_duration(7), "0:07")
        self.assertEqual(vl.fmt_duration(83.4), "1:23")
        self.assertEqual(vl.fmt_duration(3661), "1:01:01")
        self.assertIsNone(vl.fmt_duration(None))

    def test_safe_name_is_stable_and_filesystem_safe(self):
        self.assertEqual(vl.safe_name("Youtube", "0ucDb72zfrg"), "Youtube-0ucDb72zfrg")
        self.assertEqual(vl.safe_name("Youtube", "0ucDb72zfrg"),
                         vl.safe_name("Youtube", "0ucDb72zfrg"), "same link → same file")
        self.assertNotIn("/", vl.safe_name("generic", "../../etc/passwd"))
        self.assertLessEqual(len(vl.safe_name("x", "y" * 500)), 80)

    def test_summary_pulls_the_useful_facts(self):
        s = vl.summary({
            "webpage_url": "https://youtube.com/shorts/abc", "id": "abc",
            "extractor_key": "Youtube", "title": "Claim", "uploader": "Chan",
            "duration": 42, "upload_date": "20260920", "description": "x" * 5000,
        })
        self.assertEqual(s["channel"], "Chan", "uploader stands in when channel is absent")
        self.assertEqual(s["duration"], "0:42")
        self.assertEqual(s["upload_date"], "2026-09-20")
        self.assertLessEqual(len(s["description"]), vl.DESC_LIMIT + 1)

    def test_live_and_upcoming_are_both_refused(self):
        self.assertTrue(vl.is_live({"is_live": True}))
        self.assertTrue(vl.is_live({"live_status": "is_upcoming"}))
        self.assertFalse(vl.is_live({"live_status": "was_live"}), "a finished stream is fine")

    def test_partial_downloads_are_never_reused(self):
        with tempfile.TemporaryDirectory() as d:
            cwd = os.getcwd()
            os.chdir(d)
            try:
                os.mkdir(vl.MEDIA_DIR)
                Path(vl.MEDIA_DIR, "Youtube-abc.webm.part").write_bytes(b"x")
                Path(vl.MEDIA_DIR, "Youtube-abc.info.json").write_text("{}")
                self.assertIsNone(vl.cached("Youtube-abc"))
                Path(vl.MEDIA_DIR, "Youtube-abc.opus").write_bytes(b"audio")
                self.assertEqual(vl.cached("Youtube-abc"), "media/Youtube-abc.opus")
            finally:
                os.chdir(cwd)


class FetchGuards(unittest.TestCase):
    """Refusals that must happen BEFORE anything is downloaded. Driven in-process with
    a stubbed probe, because a bare file served locally reports no length and no live
    status — so these guards can't be reached through the HTTP fixture."""

    def setUp(self):
        self.ws = tempfile.mkdtemp(prefix="video-link-guard-")
        self.cwd = os.getcwd()
        os.chdir(self.ws)
        self._probe, self._download = vl.probe, vl.download
        self.downloads = []
        vl.download = self._fake_download

    def _fake_download(self, url, stem, fmt, extra):
        self.downloads.append(stem)
        Path(vl.MEDIA_DIR).mkdir(exist_ok=True)
        path = os.path.join(vl.MEDIA_DIR, stem + ".opus")
        Path(path).write_bytes(b"audio")
        return path

    def tearDown(self):
        vl.probe, vl.download = self._probe, self._download
        os.chdir(self.cwd)
        shutil.rmtree(self.ws, ignore_errors=True)

    def fetch(self, info, *argv):
        import contextlib
        import io
        vl.probe = lambda url: info
        args = vl.argparse.Namespace(url="https://example.com/v", video=False,
                                     max_height=720, max_hours=vl.DEFAULT_MAX_HOURS)
        for a in argv:
            k, v = a.split("=")
            setattr(args, k, type(getattr(args, k))(v))
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), self.assertRaises(SystemExit):
            vl.cmd_fetch(args)
        return json.loads(buf.getvalue())

    def test_an_overlong_video_is_refused_before_downloading(self):
        out = self.fetch({"id": "x", "extractor_key": "Youtube", "duration": 10 * 3600})
        self.assertEqual(out["error"], "too_long")
        self.assertEqual(out["duration"], "10:00:00")
        self.assertEqual(self.downloads, [], "nothing may be downloaded")

    def test_the_length_limit_can_be_raised_on_request(self):
        out = self.fetch({"id": "x", "extractor_key": "Youtube", "duration": 10 * 3600},
                         "max_hours=12")
        self.assertEqual(out["status"], "ok", out)
        self.assertEqual(self.downloads, ["Youtube-x"])

    def test_a_live_stream_is_refused_before_downloading(self):
        out = self.fetch({"id": "x", "extractor_key": "Youtube", "live_status": "is_live"})
        self.assertEqual(out["error"], "live")
        self.assertEqual(self.downloads, [])


class _Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


@unittest.skipUnless(HAVE_TOOLS, "needs ffmpeg, ffprobe and yt-dlp")
class Subcommands(unittest.TestCase):
    """The script as forkd runs it, against a clip served over local HTTP."""

    @classmethod
    def setUpClass(cls):
        cls.site = tempfile.mkdtemp(prefix="video-link-site-")
        subprocess.run(
            ["ffmpeg", "-v", "error", "-y",
             "-f", "lavfi", "-i", "testsrc=size=320x240:rate=15:duration=4",
             "-f", "lavfi", "-i", "sine=frequency=440:duration=4",
             "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest",
             os.path.join(cls.site, "clip.mp4")], check=True, capture_output=True)
        handler = functools.partial(_Quiet, directory=cls.site)
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        cls.url = f"http://127.0.0.1:{cls.server.server_address[1]}/clip.mp4"

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        shutil.rmtree(cls.site, ignore_errors=True)

    def setUp(self):
        self.ws = tempfile.mkdtemp(prefix="video-link-ws-")

    def tearDown(self):
        shutil.rmtree(self.ws, ignore_errors=True)

    def run_cmd(self, *args):
        p = subprocess.run([sys.executable, str(SCRIPT), *args],
                           cwd=self.ws, capture_output=True, text=True, timeout=300)
        self.assertEqual(p.returncode, 0, f"script crashed: {p.stderr[-600:]}")
        try:
            return json.loads(p.stdout)
        except json.JSONDecodeError:
            self.fail(f"stdout was not one JSON object: {p.stdout[:400]!r}")

    def test_info_reads_metadata_without_downloading(self):
        out = self.run_cmd("info", self.url)
        self.assertEqual(out["status"], "ok")
        self.assertEqual(out["id"], "clip")
        self.assertFalse(Path(self.ws, vl.MEDIA_DIR).exists(), "info must not download")

    def test_fetch_leaves_audio_only_for_transcribe(self):
        # Transcribe must never have to decode a picture — that is what made a
        # two-minute video take four minutes of CPU before (the -vn bug).
        out = self.run_cmd("fetch", self.url)
        self.assertEqual(out["status"], "ok", out)
        audio = Path(self.ws, out["audio"])
        self.assertTrue(audio.is_file())
        self.assertFalse(Path(out["audio"]).is_absolute(), "paths are workspace-relative")
        self.assertEqual(streams(audio), ["audio"])
        self.assertIsNone(out["video"])
        self.assertFalse(out["reused"])
        self.assertIn("transcribe", out["next"])

    def test_a_second_fetch_reuses_the_first_download(self):
        first = self.run_cmd("fetch", self.url)
        second = self.run_cmd("fetch", self.url)
        self.assertTrue(second["reused"])
        self.assertEqual(first["audio"], second["audio"])

    def test_fetch_with_video_also_keeps_the_picture(self):
        out = self.run_cmd("fetch", self.url, "--video")
        self.assertEqual(out["status"], "ok", out)
        self.assertEqual(streams(Path(self.ws, out["video"])), ["audio", "video"])
        self.assertEqual(streams(Path(self.ws, out["audio"])), ["audio"])

    def test_a_dead_link_is_named_not_crashed_on(self):
        out = self.run_cmd("fetch", self.url.replace("clip.mp4", "nope.mp4"))
        self.assertEqual(out["status"], "error")
        self.assertEqual(out["error"], "unavailable")


@unittest.skipUnless(LIVE, "live test — set VIDEO_LINK_LIVE=1 (needs the internet)")
class Live(unittest.TestCase):
    def test_a_real_youtube_short_comes_down_as_audio(self):
        """The canary: when this goes red, yt-dlp is stale — bump YTDLP_VERSION."""
        ws = tempfile.mkdtemp(prefix="video-link-live-")
        try:
            p = subprocess.run([sys.executable, str(SCRIPT), "fetch", LIVE_URL],
                               cwd=ws, capture_output=True, text=True, timeout=600)
            out = json.loads(p.stdout)
            self.assertEqual(out["status"], "ok", out)
            self.assertTrue(out["title"])
            self.assertEqual(streams(Path(ws, out["audio"])), ["audio"])
        finally:
            shutil.rmtree(ws, ignore_errors=True)


if __name__ == "__main__":
    unittest.main(verbosity=2)
