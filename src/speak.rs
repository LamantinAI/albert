//! Native speech — a WebRTC call to ChatGPT Voice on the subscription, the mirror of
//! `hear()` / `transcribe.rs`. `speak(text, voice) -> Blob("audio/ogg")`; the cogitator
//! emits that Blob as its reply and octo's telegram connector already sends an `audio/ogg`
//! Blob as a voice note (`sendVoice`).
//!
//! STATUS: WIP SCAFFOLD — not wired into the module tree yet (`mod speak;` is intentionally
//! absent from `main.rs`), so it does not build with the crate. It is the starting point we
//! finish and live-test against a Codex subscription. `todo!()` marks what is left.
//!
//! WHY NATIVE (str0m), AND WHY IT LOSES LESS THAN THE `tts` SKILL:
//! The `tts` skill (python/aiortc) follows a live-playout model — a late/lost packet is
//! dropped and *masked* by decoding + Opus PLC/FEC (synthesised audio, not the real data).
//! We do not play live, we RECORD to a file, so there is no playout deadline. str0m's
//! depacketizing buffer reorders and, with NACK, waits for retransmissions (its wait adapts
//! to RTT). Because a recording can wait as long as it likes, we RECOVER the real packets
//! instead of masking gaps — and remux the reordered Opus frames straight into Ogg, no
//! decode/encode. Recovery, not concealment.
//!
//! DEPENDENCIES TO ADD when wiring (Cargo.toml): `str0m` (sans-IO WebRTC), `ogg` (the Ogg
//! muxer). `reqwest` (the POST) and `serde_json` are already deps. Then add `mod speak;`.
//!
//! HOW TO TEST TOMORROW (needs the live subscription):
//!   1. De-risk the handshake first — build the offer, POST it, accept the answer, run the
//!      loop and confirm the FIRST `Event::MediaData` arrives. That is the riskiest unknown
//!      (does the ChatGPT endpoint accept a str0m offer and route media back).
//!   2. Then the Ogg muxer + the 20ms outbound silence, and confirm a playable .ogg.
//!   3. Then the NACK / reorder-wait knob and measure recovered vs lost on a real call.
//!
//! OPEN TODOs (see the inline markers):
//!   - ICE: a host candidate from our reachable addr is enough on a public-IP host; behind
//!     NAT we need a srflx candidate via a STUN round-trip (str0m is sans-IO — ours to make).
//!   - NACK: confirm str0m advertises `a=rtcp-fb:… nack` on the audio m-line and set a
//!     generous reorder/wait (latency is irrelevant for a recording).
//!   - Outbound silence: one static 20ms Opus frame, written on schedule (the call wants a mic).
//!   - Ogg: OpusHead + OpusTags, then data pages with granule = cumulative 48kHz samples.

// Reference constants and the exact call protocol live in the working skill at
// `skills/tts/scripts/tts.py` — port them here verbatim as we implement.

/// The desktop ChatGPT Voice call endpoint (client-owned WebRTC call). Same as `tts.py`.
const CALL_URL: &str = "https://chatgpt.com/backend-api/wham/realtime/calls\
                        ?intent=quicksilver&architecture=avas";
/// The only model this call accepts.
const MODEL: &str = "gpt-live-1-codex";
/// Frozen session instructions: the call starts speaking on its own, so "read this
/// verbatim" turns it into a TTS engine. The text is appended after `TEXT:\n`.
const INSTRUCTIONS: &str = "You are a text-to-speech engine. As soon as the session starts, \
    read the following text aloud verbatim, word for word, in its original language, with \
    natural intonation. Read ALL of it to the very end, then stay silent. Do not add \
    anything, do not greet, do not comment, do not summarize.\n\nTEXT:\n";
/// The voices the endpoint accepts.
const VOICES: [&str; 9] =
    ["cove", "juniper", "maple", "spruce", "ember", "vale", "breeze", "arbor", "sol"];
/// One 20ms Opus frame at 48kHz is 960 samples — the outbound-silence and granule step.
const SAMPLES_PER_FRAME: u64 = 960;

// NOTE: signatures below are the intended shape; types (`Blob`, `Subscription`, `Result`,
// `Error::Voice`) come from the crate once wired. Left as a sketch, not compiled.
/*
pub async fn speak(text: &str, voice: &str, sub: &Subscription) -> Result<Blob> {
    if !VOICES.contains(&voice) {
        return Err(Error::Voice(format!("unknown voice {voice}; use one of {VOICES:?}")));
    }

    // 1) Rtc in FRAME mode → Event::MediaData yields whole, reordered Opus frames.
    let mut rtc = Rtc::builder().set_rtp_mode(false).build(Instant::now());

    // 2) One SendRecv audio m-line (the call wants a "mic") + the oai-events data channel.
    let mut api = rtc.sdp_api();
    let mid = api.add_media(MediaKind::Audio, Direction::SendRecv, None, None, None);
    let _cid = api.add_channel(Some("oai-events".to_string()));
    let (offer, pending) = api.apply().expect("offer");

    // 3) Non-trickle ICE: bind UDP and add our reachable addr as a host candidate BEFORE we
    //    POST, so the offer already carries it. TODO srflx-via-STUN when behind NAT.
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let local = reachable_addr(&socket)?; // TODO: public/routable addr, not 0.0.0.0
    rtc.add_local_candidate(Candidate::host(local, "udp")?);

    // 4) POST {sdp: offer, session:{voice, instructions, delegation:client, model}} → answer.
    //    Headers + session shape are in tts.py (OpenAI-Alpha: quicksilver=v2, originator, etc).
    let answer = post_call(&offer.to_sdp_string(), text, voice, sub).await?;
    rtc.sdp_api().accept_answer(pending, answer)?;

    // 5) Sans-IO loop: after EVERY input, drain poll_output fully; collect Opus frames and
    //    oai-events; pump 20ms silence outbound; stop on turn.done.
    let mut ogg = OggOpus::new();               // TODO OpusHead/OpusTags + granule pages
    let mut done = false;
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut buf = vec![0u8; 2000];

    while !done && Instant::now() < deadline {
        let timeout = loop {
            match rtc.poll_output()? {
                Output::Timeout(t) => break t,
                Output::Transmit(t) => { socket.send_to(&t.contents, t.destination)?; }
                Output::Event(Event::MediaData(m)) => ogg.push_frame(&m.data), // remux, no decode
                Output::Event(Event::ChannelData(d)) => match parse_oai(&d.data) {
                    Oai::TurnDone => done = true,
                    Oai::Error(e) => return Err(Error::Voice(e)),
                    Oai::Other => {}
                },
                Output::Event(_) => {}
            }
        };
        pump_silence(&mut rtc, mid)?;           // writer.write(pt, wallclock, ts += 960, SILENCE)
        feed_socket(&socket, &mut rtc, &mut buf, timeout)?; // recv_from → Input::Receive | Timeout
    }

    Ok(Blob::new(ogg.finish(), "audio/ogg").with_filename("speech.ogg"))
}

// --- helpers to implement -------------------------------------------------------------
// async fn post_call(offer_sdp, text, voice, sub) -> Result<String>   // reqwest, tts.py headers
// fn reachable_addr(sock) -> Result<SocketAddr>                        // public/routable addr
// fn pump_silence(rtc, mid) -> Result<()>                             // one 20ms Opus silence frame
// fn feed_socket(sock, rtc, buf, timeout) -> Result<()>              // Input::Receive / Input::Timeout
// fn parse_oai(bytes) -> Oai                                          // turn.done / error / other
// struct OggOpus { ... }  push_frame(&[u8]); finish() -> Vec<u8>      // ogg crate, granule = Σ samples
*/
