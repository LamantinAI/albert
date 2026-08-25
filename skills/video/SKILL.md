---
name: video
description: >-
  Works with a video that's already in the workspace: reads its technical facts,
  pulls out still frames to look at, cuts a piece out of it, builds a small
  looping preview, lifts the audio track off it, or shrinks it so it fits back
  into a chat. Activate when asked "what's in this video", "how long is it",
  "grab a frame / screenshot from it", "cut out the bit from 1:20 to 1:40",
  "make a gif out of it", "make it smaller", "pull the sound out of it".
  For what is SAID in a video, use the `transcribe` skill instead.
---

An ffmpeg toolbox over a video sitting in the shared workspace.

**What you already have without this skill.** When a video arrives in a chat, the
connector saves the file and hands you two things: Telegram's own poster frame,
which you can see, and the workspace path in a note like ``saved to workspace path
`inbox/1712-clip.mp4` ``. So you can usually answer "what is this?" from the poster
frame alone. Reach for this skill when the question needs more than that one frame.

**What belongs to another skill.** Speech is `transcribe`'s job — it reads video
containers directly, so point it at the very same path. Don't extract audio first
just to transcribe it; that's an extra step for nothing.

## How to run it

The backend is a python script, run through **forkd**: `dispatch_to_connector`
target `"forkd"`, kind `"forkd.run"`, payload:

```
{ "skill_path": "video/scripts/video.py",
  "interpreter": "python3",
  "args": ["<subcommand>", "<path>", ...],
  "timeout_secs": 120 }
```

Paths are workspace-relative — pass exactly the path the connector reported.
The script prints ONE JSON object to stdout; parse it and work from there. Every
answer is either `{"status": "ok", ...}` or `{"status": "error", "error": "<code>",
"message": "..."}`. Timestamps are written the way a person says them: `7`, `1:23`,
`0:04.5`, `1:00:00`.

## Subcommands

**`info <path>`** — duration, size, resolution, fps, codecs, `has_audio`. Cheap;
run it first when a decision depends on length or on whether there's any sound.

**`frames <path> [--count N] [--at TS ...] [--width PX]`** — still frames as JPEGs.
Without `--at` it spreads `--count` frames (default 3) across the clip, deliberately
avoiding the very start and end, which are usually black. Returns each frame's
timestamp and path.

**`clip <path> --from TS [--to TS | --duration SECS] [--out NAME]`** — cuts a piece
out. Copies the streams, so it's instant and lossless and lands on the nearest
keyframe.

**`gif <path> [--from TS] [--duration SECS] [--fps N] [--width PX]`** — a looping
preview (default 6 s, 10 fps, 480 px wide; capped at 30 s because GIFs grow fast).

**`audio <path> [--out NAME]`** — lifts the audio track out as `.m4a`, for sending
or keeping. Errors with `no_audio` on a silent clip.

**`compress <path> [--max-mb N] [--width PX]`** — re-encodes to fit a size budget
(default 19 MB, just under Telegram's bot limit). Reports `met_target` honestly
rather than claiming success.

## Showing the result

Artifacts land in the workspace root; the JSON gives you each path. To put one in
front of the user, send it with **`chat.send_file`**: `dispatch_to_connector`
target — the telegram connector's id, kind `"chat.send_file"`, payload
`{ "path": "<path from the JSON>", "caption": "<what it shows>" }`.

One caveat worth knowing: frames you extract are files, so **you can't see them
yourself** the way you see the poster frame — you can send them to the user, and
they can send one back if they want you to look at it closely.

## Reading the errors

`not_found` — no such path; re-read the path from the message. `outside_workspace`
— the path escapes the workspace; use the reported one. `no_audio` — silent clip.
`out_of_range` — asked for a moment past the end; the message says the real length.
`bad_timestamp` / `empty_range` — the times don't parse or the end isn't after the
start. `ffmpeg_missing` — the image lacks ffmpeg, which is a deployment problem
worth reporting rather than retrying.
