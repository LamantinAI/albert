"""Unit tests for the pure parts of tts.py (no network, no aiortc)."""
import importlib.util, os, sys, unittest

spec = importlib.util.spec_from_file_location("tts", os.path.join(os.path.dirname(__file__), "tts.py"))
tts = importlib.util.module_from_spec(spec); spec.loader.exec_module(tts)


class SessionTests(unittest.TestCase):
    def test_session_is_the_desktop_shape(self):
        s = tts.session_payload("Привет.", "juniper")
        self.assertEqual(s["model"], "gpt-live-1-codex")
        self.assertEqual(s["delegation"], {"type": "client"})
        self.assertEqual(s["audio"], {"output": {"voice": "juniper"}})
        self.assertEqual(s["initial_items"], [])
        self.assertTrue(s["instructions"].endswith("TEXT:\nПривет."))

    def test_headers_carry_the_alpha_flag_and_account(self):
        h = tts.headers("tok", "acct-1")
        self.assertEqual(h["OpenAI-Alpha"], "quicksilver=v2")
        self.assertEqual(h["Authorization"], "Bearer tok")
        self.assertEqual(h["chatgpt-account-id"], "acct-1")
        self.assertNotIn("chatgpt-account-id", tts.headers("tok", None))

    def test_call_id_parsed_from_location(self):
        self.assertEqual(tts.call_id_from("/v1/realtime/calls/rtc_u24_abc"), "rtc_u24_abc")
        self.assertEqual(tts.call_id_from("/v1/realtime/calls/rtc_x?y=1"), "rtc_x")
        self.assertEqual(tts.call_id_from(None), "")

    def test_errors_are_explained(self):
        self.assertIn("albert login", tts.explain_http(401, ""))
        self.assertIn("unknown voice", tts.explain_http(403, '{"error":{"message":"Voice session access denied."}}'))
        self.assertIn("usage window", tts.explain_http(429, ""))
        self.assertTrue(tts.explain_http(500, "boom").startswith("HTTP 500"))

    def test_ogg_writer_produces_a_playable_file(self):
        import subprocess, tempfile
        pcm = bytes(48000 * 4)  # one second of stereo silence
        out = os.path.join(tempfile.mkdtemp(), "t.ogg"); tts.write_ogg(out, pcm)
        self.assertGreater(os.path.getsize(out), 100)   # a second of silence encodes tiny
        probe = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "stream=codec_name",
                                "-of", "csv=p=0", out], capture_output=True, text=True)
        self.assertEqual(probe.stdout.strip(), "opus")

    def test_voice_list_is_the_v1_set(self):
        self.assertIn("cove", tts.VOICES); self.assertNotIn("marin", tts.VOICES)


if __name__ == "__main__":
    unittest.main()
