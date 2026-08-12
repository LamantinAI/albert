---
name: transcribe
requires: subscription
description: >-
  Transcribes audio and video to text on the ChatGPT subscription — no API keys, no
  per-minute billing. Any media file (m4a, mp3, mp4, mov, wav, ogg, webm…) is split
  by pauses into chunks, transcribed in parallel, and returned as text with
  [HH:MM:SS] marks. Activate when asked to transcribe a recording, a meeting, a call,
  a dictation, a lecture, a podcast; "make text from this recording", "what's on this
  recording", "turn this audio into text", "meeting transcript". Short voice messages
  in chat are transcribed automatically — this skill is for FILES.
---

Transcribing media files through the ChatGPT dictation endpoint on the subscription token.

**When it isn't needed.** A voice message sent straight to the chat Albert hears on his
own: it already arrives as text, no need to call the skill. The skill is for files: meeting
recordings, long dictations, video, anything sent as a document.

The backend is a python script, run through **forkd**: `dispatch_to_connector` target
`"forkd"`, kind `"forkd.run"`, payload
`{ "skill_path": "transcribe/scripts/transcribe.py", "interpreter": "python3",
"args": ["<file path>", "--language", "en", "-o", "<transcript path>"],
"timeout_secs": 300 }`.

## How to work

1. **Take the file path.** A file sent to the chat is placed by the connector into the
   shared workspace, and the path is reported in text like "saved to workspace path
   `...`" — pass exactly that to the script.
2. **Run the script** through forkd. Set `--language <code>` (e.g. `en`, `ru`) when the
   language is known — without it the language is auto-detected. `-o` sets the output
   file; the default is next to the source with a `_transcript.txt` suffix.
3. **Pick `timeout_secs`.** Transcription runs at ~15–30x real time, but chunks upload in
   parallel. An hour-long recording fits in 2–4 minutes: use 300s, up to 600s for
   multi-hour files.
4. **Read the result** and answer the user to the point: a summary, or the full text, or
   an answer to their question about the recording — whatever was asked. Don't dump a wall
   of several thousand characters if they asked for the gist.

## What the script prints

`[in]` duration, `[cut]` how many chunks, `[api]` progress, `[out]` the path to the text
file, the character count and speed. The transcript is in the file, not stdout: stdout
carries only this summary.

## Limits worth knowing

- **No diarization** — the endpoint doesn't tell speakers apart. If asked "by speaker",
  say honestly it can't do that.
- **Silent truncation.** The endpoint quietly cuts an over-long chunk, so the script
  splits the recording by pauses into ~5-minute pieces. If a `[!] looks truncated in
  chunks …` appears at the end, rerun with a smaller `--chunk` (e.g. 180) and tell the
  user part of it may have been lost.
- **`[[chunk failed: …]]`** in the text — that piece didn't transcribe; rerun or warn that
  there's a gap there.
- **Privacy.** Uploaded audio stays on OpenAI's servers (asset_ttl 30 days). For a clearly
  sensitive recording, warn about this before running.
