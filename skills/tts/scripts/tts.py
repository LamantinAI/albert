#!/usr/bin/env python3
"""
Turn text into a spoken OGG/Opus file on the ChatGPT subscription — no API key, no
per-character billing. It rides the desktop ChatGPT Voice call: a WebRTC "call" to
GPT-Live whose instructions are "read this text verbatim", recorded until the model
finishes its turn.

Recording taps the raw RTP packets and decodes them with libopus itself, so a lost
packet is concealed (PLC, or the in-band FEC the next packet carries) instead of
being cut out of the audio — aiortc's own recorder drops the time of a lost packet,
which on a lossy link sounds like the voice stuttering.

  python3 tts.py "Hello there." --voice cove -o reply.ogg

The token store is Albert's own (`/data/auth.json` in the container), overridable
with ALBERT_AUTH_JSON; it falls back to ~/.codex/auth.json outside the container.
"""
import argparse, asyncio, ctypes, ctypes.util, fractions, json, os, sys, time, uuid
import urllib.error, urllib.request

URL = "https://chatgpt.com/backend-api/wham/realtime/calls?intent=quicksilver&architecture=avas"
MODEL = "gpt-live-1-codex"          # the only model this call accepts
VOICES = ("cove", "juniper", "maple", "spruce", "ember", "vale", "breeze", "arbor", "sol")
MAX_CHARS = 3000                    # 833 chars → 30 s of speech; keep one call ≈ under 2 min

# The model speaks on its own as soon as the session starts, driven by the session
# instructions — that is how the desktop app plays its greeting. Making the instructions
# "read this verbatim" turns the call into a TTS engine. Nothing else enters the session:
# instructions are frozen after start, so one text is one call.
INSTRUCTIONS = (
    "You are a text-to-speech engine. As soon as the session starts, read the following "
    "text aloud verbatim, word for word, in its original language, with natural intonation. "
    "Read ALL of it to the very end, then stay silent. Do not add anything, do not greet, "
    "do not comment, do not summarize.\n\nTEXT:\n"
)


def auth_path():
    """Albert's token store, with an override and a local-dev fallback."""
    for p in (os.environ.get("ALBERT_AUTH_JSON"), "/data/auth.json",
              os.path.expanduser("~/.codex/auth.json")):
        if p and os.path.exists(p):
            return p
    sys.exit("no auth.json found (set ALBERT_AUTH_JSON, or run `albert login`)")


def creds():
    with open(auth_path()) as f:
        t = json.load(f)["tokens"]
    return t["access_token"], t.get("account_id")


def session_payload(text, voice):
    """The `session` object of the call request. `delegation: client` is what the
    desktop sends for a client-owned call; the header must be `quicksilver=v2`."""
    return {"audio": {"output": {"voice": voice}}, "delegation": {"type": "client"},
            "initial_items": [], "instructions": INSTRUCTIONS + text, "model": MODEL}


def headers(tok, acct):
    h = {"Authorization": f"Bearer {tok}", "originator": "Codex Desktop",
         "User-Agent": "Codex Desktop", "Content-Type": "application/json",
         "OpenAI-Alpha": "quicksilver=v2", "Thread-Id": str(uuid.uuid4())}
    if acct:
        h["chatgpt-account-id"] = acct
    return h


def call_id_from(location):
    """`Location: /v1/realtime/calls/rtc_…` → `rtc_…`."""
    return (location or "").split("?")[0].rstrip("/").rsplit("/", 1)[-1]


def explain_http(code, body):
    """Map the endpoint's errors to something the agent can act on."""
    if code == 401:
        return "401 — the subscription token expired; run `albert login`."
    if code == 403 and "Voice session access denied" in body:
        return "403 Voice session access denied — usually an unknown voice; use one of " + ", ".join(VOICES)
    if code == 429:
        return "429 — the Codex usage window is exhausted; try later."
    return f"HTTP {code}: {body[:200]}"


def create_call(offer_sdp, text, voice, tok, acct):
    body = json.dumps({"sdp": offer_sdp, "session": session_payload(text, voice)}).encode()
    req = urllib.request.Request(URL, data=body, headers=headers(tok, acct), method="POST")
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return r.read().decode(), call_id_from(r.headers.get("Location")), \
                r.headers.get("x-codex-primary-used-percent")
    except urllib.error.HTTPError as e:
        sys.exit(explain_http(e.code, e.read().decode("utf-8", "replace")))


FRAME = 960          # one 20 ms Opus frame at 48 kHz
MAX_GAP = 50         # conceal up to 1 s of lost packets; beyond that it's a dead link


class OpusDecoder:
    """libopus via ctypes: the one thing PyAV's wrapper can't do is conceal a loss."""

    def __init__(self):
        name = ctypes.util.find_library("opus") or "/opt/homebrew/lib/libopus.dylib"
        try:
            self.lib = ctypes.CDLL(name)
        except OSError:
            sys.exit("libopus not found — install libopus0 (Debian/Ubuntu) or `brew install opus`")
        self.lib.opus_decoder_create.restype = ctypes.c_void_p
        self.lib.opus_decode.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int,
                                         ctypes.POINTER(ctypes.c_int16), ctypes.c_int, ctypes.c_int]
        self.lib.opus_packet_get_nb_samples.argtypes = [ctypes.c_char_p, ctypes.c_int, ctypes.c_int]
        err = ctypes.c_int()
        self.dec = self.lib.opus_decoder_create(48000, 2, ctypes.byref(err))
        if err.value:
            sys.exit(f"opus_decoder_create failed: {err.value}")
        self.buf = (ctypes.c_int16 * (5760 * 2))()

    def samples(self, pkt):
        return self.lib.opus_packet_get_nb_samples(pkt, len(pkt), 48000)

    def decode(self, pkt, fec=0, frame=FRAME):
        """`pkt=None` → PLC for one frame; `fec=1` → the FEC copy of the PREVIOUS frame
        carried in `pkt` (libopus falls back to PLC when there is none)."""
        n = self.lib.opus_decode(self.dec, pkt, len(pkt) if pkt else 0, self.buf, frame, fec)
        return ctypes.string_at(self.buf, n * 4) if n > 0 else b""


def write_ogg(out, pcm_s16_stereo_48k):
    """PCM → Ogg/Opus, mono 48 kbit/s: what Telegram plays as a voice note."""
    import av
    from av import AudioFrame
    c = av.open(out, "w")
    s = c.add_stream("libopus", rate=48000)
    s.layout = "mono"; s.bit_rate = 48000
    step = FRAME * 4; pts = 0
    for i in range(0, len(pcm_s16_stereo_48k) - step + 1, step):
        f = AudioFrame(format="s16", layout="stereo", samples=FRAME)
        f.planes[0].update(pcm_s16_stereo_48k[i:i + step])
        f.sample_rate = 48000; f.pts = pts; f.time_base = fractions.Fraction(1, 48000); pts += FRAME
        for p in s.encode(f):
            c.mux(p)
    for p in s.encode(None):
        c.mux(p)
    c.close()


async def speak(text, voice, out, max_wait):
    # aiortc is imported here so `--help` and the unit tests don't need it.
    from aiortc import MediaStreamTrack, RTCPeerConnection, RTCSessionDescription
    from aiortc.rtcrtpreceiver import RTCRtpReceiver
    from av import AudioFrame

    class Silence(MediaStreamTrack):
        """The call wants a microphone; we send 20 ms frames of nothing."""
        kind = "audio"

        def __init__(self):
            super().__init__(); self.pts = 0

        async def recv(self):
            await asyncio.sleep(0.02)
            f = AudioFrame(format="s16", layout="mono", samples=960)
            for p in f.planes:
                p.update(bytes(1920))
            f.sample_rate = 48000; f.pts = self.pts; f.time_base = fractions.Fraction(1, 48000)
            self.pts += 960
            return f

    tok, acct = creds()
    pc = RTCPeerConnection()
    dc = pc.createDataChannel("oai-events")    # JSON events: transcript, turn.done, errors
    started, done = asyncio.Event(), asyncio.Event()
    state = {"transcript": "", "error": None, "seq": None, "got": 0, "lost": 0}
    dec = OpusDecoder(); pcm = []

    # Tap the audio RTP before aiortc decodes it: sequence numbers tell us what was
    # lost, and libopus conceals it. aiortc's own decode still runs, unused.
    orig_handle = RTCRtpReceiver._handle_rtp_packet

    async def tap(receiver, packet, arrival_time_ms):
        try:
            if packet.payload:
                data = bytes(packet.payload)
                if state["seq"] is not None:
                    gap = (packet.sequence_number - state["seq"] - 1) & 0xFFFF
                    if 0 < gap < MAX_GAP:
                        for _ in range(gap - 1):
                            pcm.append(dec.decode(None))         # PLC
                        pcm.append(dec.decode(data, fec=1))      # FEC (or PLC) for the last one
                        state["lost"] += gap
                state["seq"] = packet.sequence_number; state["got"] += 1
                pcm.append(dec.decode(data, 0, dec.samples(data)))
                if not started.is_set():
                    started.set()
        except Exception as e:                                   # never break the receiver
            print(f"[!]   tap: {e!r}")
        return await orig_handle(receiver, packet, arrival_time_ms)

    RTCRtpReceiver._handle_rtp_packet = tap

    @dc.on("message")
    def on_msg(m):
        try:
            j = json.loads(m)
        except Exception:
            return
        ty = j.get("type")
        if ty == "turn.done":
            state["transcript"] = j.get("turn", {}).get("transcript", ""); done.set()
        elif ty == "error":
            state["error"] = j.get("error", {}).get("message", "?"); done.set()

    pc.addTrack(Silence())
    await pc.setLocalDescription(await pc.createOffer())
    while pc.iceGatheringState != "complete":
        await asyncio.sleep(0.05)

    t0 = time.time()
    answer, call_id, used = create_call(pc.localDescription.sdp, text, voice, tok, acct)
    print(f"[call] {call_id}  voice={voice}  quota used {used}%")
    await pc.setRemoteDescription(RTCSessionDescription(sdp=answer, type="answer"))
    try:
        await asyncio.wait_for(started.wait(), 20)
    except asyncio.TimeoutError:
        await pc.close(); sys.exit("no audio within 20 s — WebRTC did not connect")
    try:
        await asyncio.wait_for(done.wait(), max_wait)
    except asyncio.TimeoutError:
        print(f"[!]   no turn.done within {max_wait}s — the recording may be cut short")
    await asyncio.sleep(0.8)      # let the audio tail land
    await pc.close()
    if state["error"]:
        sys.exit(f"voice session error: {state['error']}")
    write_ogg(out, b"".join(pcm))
    if state["lost"]:
        print(f"[net] {state['lost']} of {state['got'] + state['lost']} packets lost — concealed")
    return state["transcript"], time.time() - t0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("text", help="what to say (or `-` to read stdin)")
    ap.add_argument("-o", "--out", default="speech.ogg", help="output .ogg (Opus)")
    ap.add_argument("--voice", default="cove", choices=VOICES)
    ap.add_argument("--max-wait", type=float, default=180, help="seconds to wait for the end of speech")
    a = ap.parse_args()

    text = sys.stdin.read() if a.text == "-" else a.text
    text = text.strip()
    if not text:
        sys.exit("nothing to say")
    if len(text) > MAX_CHARS:
        sys.exit(f"text is {len(text)} chars; max {MAX_CHARS} per call — split it into parts")
    if not a.out.lower().endswith((".ogg", ".oga", ".opus")):
        sys.exit("output must be .ogg/.oga/.opus — that is what Telegram plays as a voice note")

    print(f"[in]  {len(text)} chars")
    transcript, wall = asyncio.run(speak(text, a.voice, a.out, a.max_wait))
    size = os.path.getsize(a.out) if os.path.exists(a.out) else 0
    print(f"[out] {a.out}  {size} bytes  in {wall:.0f}s")
    if transcript:
        print(f"[said] {transcript}")


if __name__ == "__main__":
    main()
