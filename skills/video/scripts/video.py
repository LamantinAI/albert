#!/usr/bin/env python3
"""Backend for the "video" skill: an ffmpeg toolbox over a workspace video.

Albert already SEES a video's poster frame — the Telegram connector attaches the
thumbnail and names the saved path. This script is what turns that path into
answers: technical facts, still frames to look at, a trimmed piece, a small
preview, or a lighter file that fits back into a chat.

Speech is deliberately NOT handled here: the `transcribe` skill already reads
video containers, so pointing it at the same path is the whole job.

Every subcommand prints ONE JSON object to stdout:
    {"status": "ok", ...} | {"status": "error", "error": <code>, "message": ...}

Subcommands:
  info     <path>
  frames   <path> [--count N | --at TS ...] [--width PX]
  clip     <path> --from TS [--to TS | --duration SECS] [--out NAME]
  gif      <path> [--from TS] [--duration SECS] [--fps N] [--width PX] [--out NAME]
  audio    <path> [--out NAME]
  compress <path> [--max-mb N] [--width PX] [--out NAME]

Paths are workspace-relative: forkd runs skill scripts with cwd = the shared
workspace, so `inbox/1712-clip.mp4` is exactly what the connector reported.
"""
import argparse
import json
import os
import shlex
import subprocess
import sys
from pathlib import Path

# A GIF grows with every frame, so a "preview" has to stay short by default —
# ten seconds at 10 fps is already a hundred frames.
GIF_MAX_SECS = 30.0
DEFAULT_GIF_SECS = 6.0
DEFAULT_FRAMES = 3
# ffmpeg is quiet by default here: its usual banner would pollute the JSON on
# stdout, and real failures are read from the exit code plus stderr.
QUIET = ["-v", "error", "-y"]


def emit(obj):
    sys.stdout.write(json.dumps(obj, ensure_ascii=False, indent=2) + "\n")
    sys.exit(0)


def fail(code, message, **extra):
    emit({"status": "error", "error": code, "message": message, **extra})


# --- pure helpers -----------------------------------------------------------

def parse_ts(text):
    """Seconds from `SS`, `M:SS` or `H:MM:SS`, each part optionally fractional.

    Timestamps are how a person points at a moment ("from 1:20"), so the parser
    accepts what they'd actually type and refuses anything ambiguous rather than
    silently seeking to zero.
    """
    if text is None:
        return None
    raw = str(text).strip()
    if not raw:
        raise ValueError("empty timestamp")
    parts = raw.split(":")
    if len(parts) > 3:
        raise ValueError(f"not a timestamp: {raw!r}")
    total = 0.0
    for part in parts:
        if part == "" or not part.replace(".", "", 1).replace("-", "", 1).isdigit():
            raise ValueError(f"not a timestamp: {raw!r}")
        value = float(part)
        if value < 0:
            raise ValueError(f"negative timestamp: {raw!r}")
        total = total * 60 + value
    return total


def fmt_ts(secs):
    """`m:ss` (or `h:mm:ss` past an hour) — how lengths are quoted back."""
    secs = int(round(secs))
    h, m, s = secs // 3600, (secs % 3600) // 60, secs % 60
    return f"{h}:{m:02d}:{s:02d}" if h else f"{m}:{s:02d}"


def frame_times(duration, count):
    """`count` sampling points spread across a clip, biased away from the edges.

    The first and last moments of a video are usually a fade or a black frame,
    so sampling at i/(count+1) gives frames that actually show something.
    """
    if count < 1:
        raise ValueError("count must be >= 1")
    if duration <= 0:
        return [0.0] * count
    return [duration * (i + 1) / (count + 1) for i in range(count)]


def slug(name):
    """A filesystem-safe stem: keeps word characters, folds everything else."""
    keep = [c if (c.isalnum() or c in "-_") else "-" for c in name]
    return "".join(keep).strip("-") or "video"


def out_path(src, suffix, ext, explicit=None, workspace=None):
    """Where a derived artifact goes: the workspace root, named after the source.

    Outputs land beside the workspace root (like the imagegen skill) rather than
    in `inbox/`, which holds what arrived rather than what we made. An existing
    file is never clobbered — a counter is appended instead, so two runs of the
    same command leave two artifacts.
    """
    root = Path(workspace) if workspace else Path.cwd()
    if explicit:
        candidate = root / Path(explicit).name
    else:
        candidate = root / f"{slug(Path(src).stem)}-{suffix}{ext}"
    stem, ext_final = candidate.stem, candidate.suffix
    n = 2
    while candidate.exists():
        candidate = candidate.with_name(f"{stem}-{n}{ext_final}")
        n += 1
    return candidate


def rel(path, workspace=None):
    """A workspace-relative path — what `chat.send_file` and the chat want."""
    root = Path(workspace) if workspace else Path.cwd()
    try:
        return str(Path(path).resolve().relative_to(root.resolve()))
    except ValueError:
        return str(path)


# --- ffmpeg plumbing --------------------------------------------------------

def run(cmd):
    """Run an ffmpeg/ffprobe command, returning (ok, stdout, stderr)."""
    try:
        p = subprocess.run(cmd, capture_output=True, text=True)
    except FileNotFoundError:
        fail("ffmpeg_missing",
             f"`{cmd[0]}` is not installed in this image — the video skill needs ffmpeg.")
    return p.returncode == 0, p.stdout, p.stderr


def resolve_input(path):
    """The source file, confined to the workspace (cwd) that forkd hands us."""
    p = Path(path)
    resolved = (Path.cwd() / p).resolve() if not p.is_absolute() else p.resolve()
    try:
        resolved.relative_to(Path.cwd().resolve())
    except ValueError:
        fail("outside_workspace",
             f"`{path}` resolves outside the workspace; pass the path the connector reported.")
    if not resolved.is_file():
        fail("not_found", f"no such file: `{path}`")
    return resolved


def probe(path):
    """ffprobe's view of a file, as a dict — the basis of every other command."""
    ok, out, err = run([
        "ffprobe", "-v", "error", "-print_format", "json",
        "-show_format", "-show_streams", str(path),
    ])
    if not ok:
        fail("unreadable", f"ffprobe could not read the file: {err.strip()[:300]}")
    try:
        return json.loads(out)
    except json.JSONDecodeError:
        fail("unreadable", "ffprobe returned output that isn't JSON")


def summarize(path, data):
    """The human-facing facts about a clip, pulled out of ffprobe's dump."""
    fmt = data.get("format") or {}
    streams = data.get("streams") or []
    video = next((s for s in streams if s.get("codec_type") == "video"), None)
    audio = next((s for s in streams if s.get("codec_type") == "audio"), None)
    duration = float(fmt.get("duration") or (video or {}).get("duration") or 0.0)
    size = int(fmt.get("size") or 0)
    info = {
        "path": rel(path),
        "duration_secs": round(duration, 3),
        "duration": fmt_ts(duration),
        "size_bytes": size,
        "size_mb": round(size / (1024 * 1024), 2) if size else None,
        "container": (fmt.get("format_name") or "").split(",")[0] or None,
        "has_audio": audio is not None,
    }
    if video:
        info.update({
            "width": video.get("width"),
            "height": video.get("height"),
            "video_codec": video.get("codec_name"),
            "fps": fps_of(video),
        })
    if audio:
        info.update({
            "audio_codec": audio.get("codec_name"),
            "audio_channels": audio.get("channels"),
        })
    return info


def fps_of(video_stream):
    """Frame rate as a number, from ffprobe's `num/den` string."""
    raw = video_stream.get("avg_frame_rate") or video_stream.get("r_frame_rate") or ""
    if "/" not in raw:
        return None
    num, _, den = raw.partition("/")
    try:
        num, den = float(num), float(den)
    except ValueError:
        return None
    return round(num / den, 3) if den else None


# --- subcommands ------------------------------------------------------------

def cmd_info(args):
    src = resolve_input(args.path)
    emit({"status": "ok", **summarize(src, probe(src))})


def cmd_frames(args):
    src = resolve_input(args.path)
    info = summarize(src, probe(src))
    duration = info["duration_secs"]

    if args.at:
        try:
            times = [parse_ts(t) for t in args.at]
        except ValueError as e:
            fail("bad_timestamp", str(e))
        beyond = [t for t in times if duration and t > duration]
        if beyond:
            fail("out_of_range",
                 f"asked for {fmt_ts(beyond[0])} but the clip is {info['duration']} long")
    else:
        times = frame_times(duration, args.count)

    scale = ["-vf", f"scale={args.width}:-2"] if args.width else []
    frames = []
    for t in times:
        dst = out_path(src, f"frame-{fmt_ts(t).replace(':', 'm', 1).replace(':', 's')}", ".jpg")
        # -ss before -i seeks by keyframe (fast); -frames:v 1 takes a single still.
        ok, _, err = run(["ffmpeg", *QUIET, "-ss", f"{t:.3f}", "-i", str(src),
                          "-frames:v", "1", *scale, "-q:v", "3", str(dst)])
        if not ok or not dst.exists():
            fail("extract_failed", f"could not take a frame at {fmt_ts(t)}: {err.strip()[:200]}")
        frames.append({"at": fmt_ts(t), "at_secs": round(t, 3), "path": rel(dst)})
    emit({"status": "ok", "source": info["path"], "duration": info["duration"],
          "count": len(frames), "frames": frames})


def cmd_clip(args):
    src = resolve_input(args.path)
    info = summarize(src, probe(src))
    try:
        start = parse_ts(args.start) or 0.0
        end = parse_ts(args.to)
    except ValueError as e:
        fail("bad_timestamp", str(e))
    length = args.duration if args.duration else (end - start if end is not None else None)
    if length is not None and length <= 0:
        fail("empty_range", "the end of the range is not after its start")

    dst = out_path(src, "clip", Path(src).suffix or ".mp4", args.out)
    # Stream copy keeps it instant and lossless; it cuts at keyframes, which is
    # what "give me that bit" actually wants.
    cmd = ["ffmpeg", *QUIET, "-ss", f"{start:.3f}", "-i", str(src)]
    if length is not None:
        cmd += ["-t", f"{length:.3f}"]
    cmd += ["-c", "copy", "-movflags", "+faststart", str(dst)]
    ok, _, err = run(cmd)
    if not ok or not dst.exists() or dst.stat().st_size == 0:
        # Copying can fail on odd containers — fall back to a re-encode.
        cmd = ["ffmpeg", *QUIET, "-ss", f"{start:.3f}", "-i", str(src)]
        if length is not None:
            cmd += ["-t", f"{length:.3f}"]
        cmd += ["-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-c:a", "aac", str(dst)]
        ok, _, err = run(cmd)
        if not ok:
            fail("clip_failed", f"could not cut the clip: {err.strip()[:200]}")
    emit({"status": "ok", "out": rel(dst), "from": fmt_ts(start),
          "duration": fmt_ts(length) if length is not None else info["duration"],
          "size_mb": round(dst.stat().st_size / (1024 * 1024), 2)})


def cmd_gif(args):
    src = resolve_input(args.path)
    try:
        start = parse_ts(args.start) or 0.0
    except ValueError as e:
        fail("bad_timestamp", str(e))
    length = min(args.duration or DEFAULT_GIF_SECS, GIF_MAX_SECS)
    dst = out_path(src, "preview", ".gif", args.out)
    # A shared palette is the difference between a clean GIF and a muddy one.
    chain = (f"fps={args.fps},scale={args.width}:-1:flags=lanczos,"
             f"split[a][b];[a]palettegen[p];[b][p]paletteuse")
    ok, _, err = run(["ffmpeg", *QUIET, "-ss", f"{start:.3f}", "-t", f"{length:.3f}",
                      "-i", str(src), "-filter_complex", chain, "-loop", "0", str(dst)])
    if not ok or not dst.exists():
        fail("gif_failed", f"could not build the preview: {err.strip()[:200]}")
    emit({"status": "ok", "out": rel(dst), "from": fmt_ts(start),
          "duration": fmt_ts(length), "fps": args.fps, "width": args.width,
          "size_mb": round(dst.stat().st_size / (1024 * 1024), 2)})


def cmd_audio(args):
    src = resolve_input(args.path)
    if not summarize(src, probe(src))["has_audio"]:
        fail("no_audio", "this clip has no audio track")
    dst = out_path(src, "audio", ".m4a", args.out)
    # Copy the existing AAC when there is one; otherwise transcode.
    ok, _, _ = run(["ffmpeg", *QUIET, "-i", str(src), "-vn", "-c:a", "copy", str(dst)])
    if not ok or not dst.exists() or dst.stat().st_size == 0:
        ok, _, err = run(["ffmpeg", *QUIET, "-i", str(src), "-vn",
                          "-c:a", "aac", "-b:a", "128k", str(dst)])
        if not ok:
            fail("audio_failed", f"could not extract the audio: {err.strip()[:200]}")
    emit({"status": "ok", "out": rel(dst),
          "size_mb": round(dst.stat().st_size / (1024 * 1024), 2),
          "hint": "for speech, run the `transcribe` skill on this file or on the video itself"})


def cmd_compress(args):
    src = resolve_input(args.path)
    info = summarize(src, probe(src))
    duration = info["duration_secs"]
    if duration <= 0:
        fail("unreadable", "the clip has no measurable duration to size a bitrate against")
    dst = out_path(src, "small", ".mp4", args.out)
    # Budget the video bitrate from the size target, leaving room for audio and
    # container overhead, so the result actually lands under the limit.
    audio_kbps = 96 if info["has_audio"] else 0
    total_kbps = (args.max_mb * 8 * 1024) / duration
    video_kbps = max(int(total_kbps * 0.92) - audio_kbps, 120)
    cmd = ["ffmpeg", *QUIET, "-i", str(src),
           "-vf", f"scale='min({args.width},iw)':-2",
           "-c:v", "libx264", "-preset", "veryfast", "-b:v", f"{video_kbps}k",
           "-maxrate", f"{int(video_kbps * 1.5)}k", "-bufsize", f"{video_kbps * 2}k",
           "-movflags", "+faststart"]
    cmd += ["-c:a", "aac", "-b:a", f"{audio_kbps}k"] if audio_kbps else ["-an"]
    ok, _, err = run(cmd + [str(dst)])
    if not ok or not dst.exists():
        fail("compress_failed", f"could not compress: {err.strip()[:200]}")
    size_mb = round(dst.stat().st_size / (1024 * 1024), 2)
    emit({"status": "ok", "out": rel(dst), "size_mb": size_mb,
          "was_mb": info["size_mb"], "target_mb": args.max_mb,
          "met_target": size_mb <= args.max_mb})


def main():
    ap = argparse.ArgumentParser(description="ffmpeg toolbox for the video skill")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("info"); p.add_argument("path")

    p = sub.add_parser("frames")
    p.add_argument("path")
    p.add_argument("--count", type=int, default=DEFAULT_FRAMES)
    p.add_argument("--at", action="append", help="explicit timestamp; repeatable")
    p.add_argument("--width", type=int, default=None)

    p = sub.add_parser("clip")
    p.add_argument("path")
    p.add_argument("--from", dest="start", required=True)
    p.add_argument("--to", default=None)
    p.add_argument("--duration", type=float, default=None)
    p.add_argument("--out", default=None)

    p = sub.add_parser("gif")
    p.add_argument("path")
    p.add_argument("--from", dest="start", default=None)
    p.add_argument("--duration", type=float, default=None)
    p.add_argument("--fps", type=int, default=10)
    p.add_argument("--width", type=int, default=480)
    p.add_argument("--out", default=None)

    p = sub.add_parser("audio"); p.add_argument("path"); p.add_argument("--out", default=None)

    p = sub.add_parser("compress")
    p.add_argument("path")
    p.add_argument("--max-mb", type=float, default=19.0)
    p.add_argument("--width", type=int, default=1280)
    p.add_argument("--out", default=None)

    args = ap.parse_args()
    {"info": cmd_info, "frames": cmd_frames, "clip": cmd_clip,
     "gif": cmd_gif, "audio": cmd_audio, "compress": cmd_compress}[args.cmd](args)


if __name__ == "__main__":
    main()
