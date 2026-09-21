---
name: tts
requires: subscription
description: >-
  Speaks: turns text into a voice message on the ChatGPT subscription — no API keys,
  no per-character billing. Produces an OGG/Opus file that goes to the chat as a
  play-in-place voice note. Activate when asked to "say it out loud", "send a voice
  message", "read this to me", "voice reply", "answer with your voice", "record
  this as audio", or when the user asks to hear a text (a note, a summary, a
  reminder) rather than read it.
---

Text-to-speech through the ChatGPT Voice call, on the subscription token Albert
already holds. One call reads one text aloud, verbatim; the result is an `.ogg`.

The backend is a python script, run through **forkd**: `dispatch_to_connector` target
`"forkd"`, kind `"forkd.run"`, payload
`{ "skill_path": "tts/scripts/tts.py", "interpreter": "python3",
"args": ["<text>", "--voice", "cove", "-o", "<name>.ogg"], "timeout_secs": 240 }`.

## How to work

1. **Write the text as it should sound.** The engine reads exactly what it gets: expand
   abbreviations, drop Markdown (`**`, `#`, links, tables), spell numbers the way they
   are spoken if the reading matters ("2:30 pm" → "two thirty pm"). Keep it
   under **3000 characters** per call (≈ 2 minutes of speech); a longer text goes as
   several calls, one voice note each, in order.
2. **Run the script.** Pass the text as the first argument (or `-` and put the text on
   `stdin` for anything with quotes or newlines). `--voice`: `cove` (default, calm),
   `juniper`, `ember`, `sol`, `maple`, `spruce`, `vale`, `breeze`, `arbor`. `-o` names the
   workspace file; keep the `.ogg` extension — that is what Telegram plays as a voice
   note.
3. **Pick `timeout_secs`.** Speech is produced in real time plus ~2 s of setup: a
   short sentence is done in ~7 s, 800 characters in ~35 s. Use 240 s.
4. **Deliver it** with `chat.send_file` (target — the telegram connector's id, kind
   `"chat.send_file"`, payload `{ "path": "<name>.ogg", "caption": "<optional>" }`). An
   `.ogg` goes out as a **voice note**, not a document. Don't paste the transcript in
   the reply unless asked — the voice is the reply.

## What the script prints

`[in]` character count, `[call]` the call id, voice and the subscription usage so far,
`[out]` the path, size and wall time, `[said]` what the model actually read (its own
transcript). If `[said]` differs from the text in a way that matters, say so.

## Limits worth knowing

- **One text = one call.** Nothing can be added to a running session, so there is no
  "continue" — split long texts up front.
- **It is a language model reading, not a classic TTS.** It reads verbatim in
  practice (checked on Russian and English), but a text that reads like an instruction
  to the model ("answer the following…") may be answered instead of read. Keep the
  text declarative.
- **Usage.** Each call draws on the same Codex usage window the agent itself uses
  (≈ 0.5 % of a Plus window per call). `429` means the window is exhausted — tell the
  user when it resets rather than retrying in a loop.
- **Errors.** `401` — the subscription token expired (`albert login`); `403 Voice session
  access denied` — an unknown voice name; "no audio track within 20 s" — the WebRTC
  leg didn't connect (network), retry once, then report.
- **Privacy.** The text is sent to OpenAI's voice servers, like any chat message.
