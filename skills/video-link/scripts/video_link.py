#!/usr/bin/env python3
"""Backend for the "video-link" skill: turn a link to a video into something readable.

A link to a video (YouTube, Shorts, youtu.be, VK, Rutube, TikTok — whatever yt-dlp
supports) tells a model almost nothing on its own: opening the page shows a title and
not one word of what is said. This script downloads the AUDIO into the workspace, so
the `transcribe` skill can turn it into text, and reads the video's metadata (title,
channel, length, description) on the way.

Every subcommand prints ONE JSON object to stdout:
    {"status": "ok", ...} | {"status": "error", "error": <code>, "message": ...}

Subcommands:
  info  <url>                            metadata only, nothing is downloaded
  fetch <url> [--video] [--max-height N] [--max-hours H]
                                         audio (and, with --video, the picture) into media/

Paths are workspace-relative: forkd runs skill scripts with cwd = the shared workspace.
"""
import argparse
import glob
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

MEDIA_DIR = "media"
# The description rides back to the model in full; long ones are mostly link lists and
# sponsor blurbs, so cap it where it stops carrying signal.
DESC_LIMIT = 1500
# Hours of audio we agree to pull by default. Transcribing is cheap on the subscription,
# but a ten-hour stream is almost never what "check this video" meant.
DEFAULT_MAX_HOURS = 4.0
INFO_TIMEOUT = 90
DOWNLOAD_TIMEOUT = 900
# Partial and side files yt-dlp leaves next to a download; never mistake one for media.
NOT_MEDIA = (".part", ".ytdl", ".json", ".temp", ".tmp")

# How yt-dlp words its failures, mapped to a code the model can act on. Order matters —
# first match wins: YouTube's age gate also says "sign in to confirm", so it must be
# tested before the bot check.
ERROR_PATTERNS = [
    ("private", ("private video", "this video is private")),
    ("age_restricted", ("confirm your age", "age-restricted", "age restricted",
                        "inappropriate for some users")),
    ("members_only", ("members-only", "members only", "join this channel")),
    ("bot_check", ("not a bot", "sign in to confirm")),
    # YouTube words it several ways ("not available in your country", "has not made
    # this video available in your country", "blocked it in your country").
    ("geo_blocked", ("in your country", "geo restrict", "geo-restrict")),
    ("live", ("is live", "live event", "premieres in", "this live event")),
    ("unavailable", ("video unavailable", "has been removed", "no longer available",
                     "does not exist", "http error 404", "404: not found")),
    ("unsupported_url", ("unsupported url",)),
    ("forbidden", ("http error 403", "403: forbidden")),
    ("no_formats", ("requested format is not available", "no video formats found")),
]

HINTS = {
    "private": "The video is private — only its owner can see it.",
    "age_restricted": "The video is age-restricted and needs a signed-in account.",
    "members_only": "The video is for channel members only.",
    "bot_check": "The site asked to prove we are not a bot (it flags server traffic). "
                 "Retry later; if it keeps happening, the server IP is being blocked.",
    "geo_blocked": "The video is not available from the server's country.",
    "live": "This is a live stream or a premiere; there is no finished recording yet.",
    "unavailable": "The video is gone or the link is wrong.",
    "unsupported_url": "This link isn't a video yt-dlp knows how to fetch.",
    "forbidden": "The site refused the download (HTTP 403). yt-dlp is probably out of "
                 "date — its version is pinned in the image and needs a bump.",
    "no_formats": "The site offered no downloadable formats. yt-dlp is probably out of "
                  "date — its version is pinned in the image and needs a bump.",
    "download_failed": "yt-dlp failed; the stderr tail says why.",
}


def emit(obj):
    sys.stdout.write(json.dumps(obj, ensure_ascii=False, indent=2) + "\n")
    sys.exit(0)


def fail(code, message, **extra):
    emit({"status": "error", "error": code, "message": message, **extra})


# --- pure helpers -----------------------------------------------------------

def classify_error(stderr):
    """Map yt-dlp's stderr to an error code (see ERROR_PATTERNS)."""
    text = (stderr or "").lower()
    for code, needles in ERROR_PATTERNS:
        if any(n in text for n in needles):
            return code
    return "download_failed"


def fmt_duration(secs):
    """`m:ss`, or `h:mm:ss` once past an hour; `None` when the length is unknown."""
    if secs is None:
        return None
    secs = int(round(float(secs)))
    h, m, s = secs // 3600, (secs % 3600) // 60, secs % 60
    return f"{h}:{m:02d}:{s:02d}" if h else f"{m}:{s:02d}"


def safe_name(extractor, video_id):
    """A filesystem-safe, stable stem for one video: the same link always maps to the
    same file, which is what lets a second request reuse the first download."""
    raw = f"{extractor or 'video'}-{video_id or 'unknown'}"
    return re.sub(r"[^A-Za-z0-9_-]+", "_", raw).strip("_")[:80] or "video"


def trim(text, limit=DESC_LIMIT):
    text = (text or "").strip()
    return text if len(text) <= limit else text[:limit].rstrip() + "…"


def fmt_date(yyyymmdd):
    """yt-dlp's `upload_date` (YYYYMMDD) as YYYY-MM-DD."""
    if not yyyymmdd or not re.fullmatch(r"\d{8}", str(yyyymmdd)):
        return None
    d = str(yyyymmdd)
    return f"{d[:4]}-{d[4:6]}-{d[6:]}"


def summary(data):
    """The facts about a video worth handing to the model, out of yt-dlp's info dict."""
    duration = data.get("duration")
    return {
        "url": data.get("webpage_url") or data.get("original_url"),
        "id": data.get("id"),
        "site": data.get("extractor_key") or data.get("extractor"),
        "title": data.get("title"),
        "channel": data.get("channel") or data.get("uploader"),
        "duration_secs": duration,
        "duration": fmt_duration(duration),
        "upload_date": fmt_date(data.get("upload_date")),
        "view_count": data.get("view_count"),
        "description": trim(data.get("description")) or None,
    }


def is_live(data):
    return bool(data.get("is_live")) or data.get("live_status") in ("is_live", "is_upcoming")


def rel(path):
    """A workspace-relative path — what `transcribe` and `chat.send_file` take."""
    try:
        return os.path.relpath(path, Path.cwd())
    except ValueError:
        return str(path)


def cached(stem):
    """A finished download for this stem, if one is already in media/."""
    for p in sorted(glob.glob(os.path.join(MEDIA_DIR, glob.escape(stem) + ".*"))):
        if not p.endswith(NOT_MEDIA) and os.path.getsize(p) > 0:
            return p
    return None


# --- yt-dlp plumbing ----------------------------------------------------------

def ytdlp_bin():
    """yt-dlp from PATH (baked into the image), else a user-level install in the
    workspace — how the agent got one before the image shipped it."""
    found = shutil.which("yt-dlp")
    if found:
        return found
    local = Path.home() / ".local" / "bin" / "yt-dlp"
    if local.is_file() and os.access(local, os.X_OK):
        return str(local)
    return None


def run_ytdlp(args, timeout):
    exe = ytdlp_bin()
    if not exe:
        fail("ytdlp_missing", "yt-dlp is not installed in this image.")
    try:
        p = subprocess.run([exe, *args], capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        fail("timeout", f"yt-dlp did not finish within {timeout}s.")
    return p


def failed(p, url):
    code = classify_error(p.stderr)
    tail = "\n".join(line for line in (p.stderr or "").splitlines()
                     if "ERROR" in line or "WARNING" in line)[-600:]
    fail(code, HINTS[code], url=url, detail=tail or (p.stderr or "")[-600:])


def probe(url):
    """yt-dlp's info dict for one video, without downloading it."""
    p = run_ytdlp(["-J", "--no-playlist", "--no-warnings", url], INFO_TIMEOUT)
    if p.returncode != 0:
        failed(p, url)
    try:
        data = json.loads(p.stdout)
    except json.JSONDecodeError:
        fail("download_failed", "yt-dlp returned something that isn't JSON.", url=url)
    if data.get("_type") == "playlist":
        fail("playlist", "This is a playlist or a channel, not one video — send a link "
                         "to a single video.", url=url, title=data.get("title"))
    return data


def download(url, stem, fmt, extra):
    """Download `url` in format `fmt` to media/<stem>.<ext>; return the final path."""
    Path(MEDIA_DIR).mkdir(exist_ok=True)
    p = run_ytdlp([
        "--no-playlist", "--no-progress", "--no-simulate",
        # The path after post-processing, so the caller knows the real extension.
        "--print", "after_move:filepath",
        "-f", fmt, *extra,
        "-o", os.path.join(MEDIA_DIR, stem + ".%(ext)s"),
        url,
    ], DOWNLOAD_TIMEOUT)
    if p.returncode != 0:
        failed(p, url)
    lines = [ln.strip() for ln in (p.stdout or "").splitlines() if ln.strip()]
    path = lines[-1] if lines else cached(stem)
    if not path or not os.path.isfile(path):
        fail("download_failed", "yt-dlp reported success but no file appeared.", url=url)
    return path


# --- subcommands -------------------------------------------------------------

def cmd_info(args):
    data = probe(args.url)
    emit({"status": "ok", **summary(data), "live": is_live(data)})


def cmd_fetch(args):
    data = probe(args.url)
    if is_live(data):
        fail("live", HINTS["live"], url=args.url, title=data.get("title"))
    duration = data.get("duration")
    if duration and duration > args.max_hours * 3600:
        fail("too_long", f"The video is {fmt_duration(duration)} long, over the "
                         f"{args.max_hours:g} h limit. Pass a larger --max-hours if the "
                         f"whole thing is really wanted.", url=args.url,
             title=data.get("title"), duration=fmt_duration(duration))

    stem = safe_name(data.get("extractor_key") or data.get("extractor"), data.get("id"))
    audio = cached(stem)
    reused = audio is not None
    if not reused:
        # Audio only, whatever the site offers; -x re-muxes it into a plain audio file,
        # and for a single progressive file (no separate audio stream) it strips the
        # picture, so `transcribe` never has to decode video.
        audio = download(args.url, stem, "bestaudio/best", ["-x", "--audio-format", "best"])

    video = None
    if args.video:
        vstem = stem + "-video"
        video = cached(vstem) or download(
            args.url, vstem,
            f"bv*[height<={args.max_height}]+ba/b[height<={args.max_height}]/best",
            ["--merge-output-format", "mp4"])

    emit({
        "status": "ok",
        **summary(data),
        "audio": rel(audio),
        "audio_mb": round(os.path.getsize(audio) / (1024 * 1024), 2),
        "video": rel(video) if video else None,
        "reused": reused,
        "next": "run the `transcribe` skill on `audio` to get what is said",
    })


def main():
    ap = argparse.ArgumentParser(description="Fetch a video from a link for the video-link skill")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("info")
    p.add_argument("url")

    p = sub.add_parser("fetch")
    p.add_argument("url")
    p.add_argument("--video", action="store_true",
                   help="also download the picture (for stills via the `video` skill)")
    p.add_argument("--max-height", type=int, default=720)
    p.add_argument("--max-hours", type=float, default=DEFAULT_MAX_HOURS)

    args = ap.parse_args()
    {"info": cmd_info, "fetch": cmd_fetch}[args.cmd](args)


if __name__ == "__main__":
    main()
