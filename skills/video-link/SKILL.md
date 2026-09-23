---
name: video-link
command: watch
command_about: What's said in a video: /watch <link>
description: >-
  Reads a video from a LINK so you can answer about what is SAID in it: downloads the
  audio of a YouTube video, Short or youtu.be link (also VK, Rutube, TikTok, Dzen —
  anything yt-dlp supports) into the workspace, with its title, channel, length and
  description, ready for the `transcribe` connector. Activate whenever a message contains
  a link to a video — "is this true?", "what is this video about", "summarize it",
  "fact-check this", "transcribe this video", or a bare link with no question.
  Do NOT open video links with the browser: a video page shows a title, not a word of
  what is said.
---

A link to a video is opaque until you hear it. This skill fetches the audio and the
metadata; the `transcribe` connector turns the audio into text; then you answer from what was
actually said.

## How to run it

The backend is a python script, run through **forkd**: `dispatch_to_connector` target
`"forkd"`, kind `"forkd.run"`, payload:

```
{ "skill_path": "video-link/scripts/video_link.py",
  "interpreter": "python3",
  "args": ["fetch", "<the link>"],
  "timeout_secs": 900 }
```

It prints ONE JSON object: `{"status": "ok", ...}` or `{"status": "error", "error":
"<code>", "message": "..."}`. On success you get `title`, `channel`, `duration`,
`upload_date`, `description` and `audio` — a workspace path. The same link fetched
twice reuses the first download (`"reused": true`).

- `info <link>` — metadata only, nothing downloaded. Enough when the question is just
  "what is this video" and the title answers it.
- `fetch <link>` — the audio. Add `--video` to also keep the picture (≤720p) when you
  need stills: hand that path to the `video` skill's `frames`.
- `--max-hours H` — videos over 4 h are refused by default; raise it only when the
  user really wants a long recording.

## The whole job, step by step

1. **Fetch.** Run `fetch` on the link.
2. **Transcribe.** Dispatch `transcribe.run { path: <the audio path from step 1> }` to the
   "transcribe" connector. A long recording comes back split on its pauses, with
   `[hh:mm:ss]` timecodes. Check `failed` and `truncated_chunks` in the result and look for
   `[[chunk` in the text — a failed chunk is a hole in the text, not a quiet pause; say so
   rather than answer around it.
3. **Answer from what was said**, not from the title:
   - "What is it about / summarize" → a short summary with the key points.
   - "Is this true / fact-check" → pull out the concrete claims (who, what, numbers,
     dates), check each against the web with the `search` connector, and give a
     verdict per claim with its sources. When something can't be verified, say that
     plainly instead of guessing. The title and description are the author's framing,
     not evidence.
   - "Transcribe it" → write the transcript to a workspace file (your file tools) and send
     it with `chat.send_file`.
4. If the video has no speech (music, silence), say so; the title and description are
   all there is, and stills from `--video` go to the user — you can't see extracted
   frames yourself.

## Reading the errors

| error | what it means / what to do |
|---|---|
| `unavailable` | the video is gone or the link is wrong — ask for another link |
| `private`, `members_only`, `age_restricted` | the video needs an account; can't be fetched |
| `geo_blocked` | not available from the server's country |
| `live` | a stream or a premiere with no finished recording yet |
| `playlist` | the link is a playlist or a channel — ask for one video |
| `too_long` | over the length limit — confirm before raising `--max-hours` |
| `bot_check` | the site challenged the server's IP; retry later |
| `forbidden`, `no_formats` | yt-dlp is probably out of date — tell the owner the pinned version needs a bump |
| `timeout`, `download_failed` | read `detail`; retry once, then report it |

Report an error in one plain sentence and, if it helps, ask the user for a screenshot
or a quote of the claim — but only after the fetch has actually failed.
