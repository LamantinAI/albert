#!/usr/bin/env python3
"""
Transcribe any media file via ChatGPT's dictation endpoint, on the subscription
token Albert already holds. Splits on silence into chunks, uploads them in
parallel, merges the result with timecodes derived from each chunk's offset.

  python3 transcribe.py meeting.m4a --language ru -o out.txt

The token store is Albert's own (`/data/auth.json` in the container), overridable
with ALBERT_AUTH_JSON; it falls back to ~/.codex/auth.json outside the container.
"""
import argparse, concurrent.futures as cf, json, os, re, subprocess, sys, tempfile, time, uuid
import urllib.request, urllib.error

URL = "https://chatgpt.com/backend-api/transcribe"
HARD_LIMIT = 1380          # 23 min proven OK; 24 min returns 500


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


def duration(path):
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration",
         "-of", "csv=p=0", path], capture_output=True, text=True, check=True).stdout
    return float(out.strip())


def silences(path, noise="-30dB", min_len=0.6):
    """Return silence midpoints (seconds) as candidate cut points."""
    p = subprocess.run(
        ["ffmpeg", "-v", "info", "-i", path,
         "-af", f"silencedetect=noise={noise}:d={min_len}", "-f", "null", "-"],
        capture_output=True, text=True)
    starts = [float(m) for m in re.findall(r"silence_start: ([0-9.]+)", p.stderr)]
    ends = [float(m) for m in re.findall(r"silence_end: ([0-9.]+)", p.stderr)]
    return sorted((s + e) / 2 for s, e in zip(starts, ends))


def plan_cuts(total, cuts, target, hard):
    """Greedy: from each position, take the last silence before `target`.

    Every chunk stays near `target` — including the final one. The endpoint
    accepts up to ~23 min of audio but silently truncates the transcript well
    before that, so oversized chunks lose speech without any error.
    """
    bounds, pos = [0.0], 0.0
    while total - pos > target * 1.4:
        window = [c for c in cuts if pos + target * 0.5 < c <= pos + target]
        if not window:
            window = [c for c in cuts if pos + target < c <= pos + min(target * 1.4, hard)]
        nxt = window[-1] if window else min(pos + target, total)
        if nxt <= pos:
            nxt = min(pos + target, total)
        bounds.append(nxt)
        pos = nxt
    bounds.append(total)
    return [(bounds[i], bounds[i + 1]) for i in range(len(bounds) - 1)
            if bounds[i + 1] - bounds[i] > 0.5]


def cut(path, start, end, outdir, idx):
    out = os.path.join(outdir, f"chunk_{idx:03d}.webm")
    subprocess.run(
        ["ffmpeg", "-y", "-v", "error", "-ss", f"{start:.3f}", "-t", f"{end-start:.3f}",
         "-i", path, "-ac", "1", "-c:a", "libopus", "-b:a", "24k", out], check=True)
    return out


def post(path, language, tok, acct, retries=3):
    body = bytearray()
    b = f"----codex-transcribe-{uuid.uuid4()}"
    body += f"--{b}\r\n".encode()
    body += b'Content-Disposition: form-data; name="file"; filename="codex.webm"\r\n'
    body += b"Content-Type: audio/webm\r\n\r\n"
    body += open(path, "rb").read()
    body += b"\r\n"
    if language:
        body += f"--{b}\r\n".encode()
        body += b'Content-Disposition: form-data; name="language"\r\n\r\n'
        body += f"{language}\r\n".encode()
    body += f"--{b}--\r\n".encode()

    hdrs = {"Authorization": f"Bearer {tok}",
            "Content-Type": f"multipart/form-data; boundary={b}",
            "originator": "Codex Desktop", "User-Agent": "Codex Desktop"}
    if acct:
        hdrs["chatgpt-account-id"] = acct

    last = None
    for attempt in range(retries):
        try:
            req = urllib.request.Request(URL, data=bytes(body), method="POST", headers=hdrs)
            with urllib.request.urlopen(req, timeout=300) as r:
                return json.loads(r.read().decode("utf-8", "replace")).get("text", "")
        except urllib.error.HTTPError as e:
            last = f"HTTP {e.code}: {e.read().decode('utf-8','replace')[:200]}"
            if e.code == 401:
                raise SystemExit("401 — токен подписки протух; нужен `albert login`.")
            if e.code < 500:
                break
        except Exception as e:
            last = f"{type(e).__name__}: {e}"
        time.sleep(2 * (attempt + 1))
    return f"[[chunk failed: {last}]]"


def hms(s):
    s = int(s)
    return f"{s//3600:02d}:{s%3600//60:02d}:{s%60:02d}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("file")
    ap.add_argument("-o", "--out", default=None)
    ap.add_argument("--language", default=None, help="ru, en, … (optional)")
    ap.add_argument("--chunk", type=float, default=300, help="target chunk seconds (default 300)")
    ap.add_argument("--jobs", type=int, default=4, help="parallel uploads")
    ap.add_argument("--keep", action="store_true", help="keep chunk files")
    a = ap.parse_args()

    if a.chunk > HARD_LIMIT:
        sys.exit(f"--chunk must be <= {HARD_LIMIT}s (endpoint rejects ~24min+)")

    tok, acct = creds()
    total = duration(a.file)
    print(f"[in]  {a.file}  {hms(total)}")

    t0 = time.time()
    cuts = silences(a.file)
    segs = plan_cuts(total, cuts, a.chunk, HARD_LIMIT)
    print(f"[cut] {len(cuts)} пауз найдено → {len(segs)} чанков "
          f"(медиана {sorted(e-s for s,e in segs)[len(segs)//2]:.0f}s)")

    tmp = tempfile.mkdtemp(prefix="tx_")
    files = [cut(a.file, s, e, tmp, i) for i, (s, e) in enumerate(segs)]

    texts = [None] * len(files)
    done = 0
    with cf.ThreadPoolExecutor(max_workers=a.jobs) as ex:
        futs = {ex.submit(post, f, a.language, tok, acct): i for i, f in enumerate(files)}
        for fut in cf.as_completed(futs):
            i = futs[fut]
            texts[i] = fut.result()
            done += 1
            print(f"\r[api] {done}/{len(files)} чанков", end="", flush=True)
    print()

    lines = [f"[{hms(s)}] {t.strip()}" for (s, _), t in zip(segs, texts) if t and t.strip()]
    body = "\n\n".join(lines)

    out = a.out or os.path.splitext(a.file)[0] + "_transcript.txt"
    with open(out, "w") as f:
        f.write(body + "\n")

    wall = time.time() - t0
    fails = sum(1 for t in texts if t and t.startswith("[[chunk failed"))
    # a chunk that ends without sentence-final punctuation was almost certainly cut off
    trunc = [i for i, t in enumerate(texts)
             if t and not t.startswith("[[chunk") and t.strip()[-1:] not in ".!?…»\"'"]
    print(f"[out] {out}  {len(body)} знаков  за {wall:.0f}s = {total/wall:.1f}x реального времени"
          + (f"  ОШИБОК: {fails}" if fails else ""))
    if trunc:
        print(f"[!]   похоже на обрыв в чанках {trunc} — уменьши --chunk")
    if not a.keep:
        for f in files:
            os.unlink(f)
        os.rmdir(tmp)
    else:
        print(f"[tmp] чанки: {tmp}")


main()
