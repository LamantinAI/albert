#!/usr/bin/env bash
# imagegen — generate one image via Codex's built-in image_gen tool, which runs on the
# shared ChatGPT SUBSCRIPTION (no OPENAI_API_KEY, no per-image billing). We drive
# `codex exec` non-interactively and collect the PNG it saves under
# $CODEX_HOME/generated_images, copying it into the workspace.
#
# Usage:
#   imagegen.sh --prompt "<full spec>" [--out PATH] [--size WxH] [--image FILE]...
#
# --prompt : the built prompt/spec (see the skill for how to shape it). Required.
# --out    : where to write the final PNG (default: ./image_<epoch>.png in the cwd,
#            which forkd sets to the workspace).
# --size   : gpt-image-2 size hint, e.g. 1024x1024, 1536x1024, 1024x1536, auto.
# --image  : an input image (reference or edit target), repeatable. Passed to codex
#            with -i; describe each image's role inside --prompt.
#
# Prints a short summary to stdout ([out] <path>, [size], [prompt]); the image is the
# file, not stdout. Non-zero exit + a message on stderr on failure.
set -euo pipefail

prompt=""
out=""
size=""
images=()

while [ $# -gt 0 ]; do
  case "$1" in
    --prompt) prompt="${2:-}"; shift 2 ;;
    --out)    out="${2:-}"; shift 2 ;;
    --size)   size="${2:-}"; shift 2 ;;
    --image)  images+=("${2:-}"); shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

[ -n "$prompt" ] || { echo "--prompt is required" >&2; exit 2; }

# Locate codex: PATH first (the deploy installs it at /usr/local/bin/codex), then a
# couple of common spots.
codex_bin="$(command -v codex 2>/dev/null || true)"
if [ -z "$codex_bin" ]; then
  for c in /usr/local/bin/codex "$HOME/.local/bin/codex"; do
    [ -x "$c" ] && { codex_bin="$c"; break; }
  done
fi
[ -n "$codex_bin" ] || { echo "codex binary not found (PATH or /usr/local/bin). Image generation needs the Codex CLI + a ChatGPT-subscription auth.json." >&2; exit 3; }

CODEX_HOME="${CODEX_HOME:-$HOME/.codex}"
gen_dir="$CODEX_HOME/generated_images"

# Confirm we are on the subscription, not a billed API key.
auth="$CODEX_HOME/auth.json"
if [ -r "$auth" ] && command -v python3 >/dev/null 2>&1; then
  mode="$(python3 -c "import json,sys;print(json.load(open('$auth')).get('auth_mode',''))" 2>/dev/null || true)"
  if [ -n "$mode" ] && [ "$mode" != "chatgpt" ]; then
    echo "codex auth_mode is '$mode', not 'chatgpt' (subscription). Refusing to run so we don't bill an API key." >&2
    exit 4
  fi
fi

[ -z "$out" ] && out="./image_$(date +%s).png"
mkdir -p "$(dirname "$out")"

# Marker so we can pick out the image produced by THIS run (mtime-newer-than).
marker="$(mktemp)"; trap 'rm -f "$marker"' EXIT

# Build the instruction. Force the built-in tool and a single image; forbid the billed
# CLI/API path; demand the saved path back as the only reply.
instruction="Use your built-in image_gen tool to generate EXACTLY ONE image from the specification below. \
Do NOT use the CLI fallback, scripts/image_gen.py, the OpenAI API, or write any code — only the built-in image_gen tool. \
Do not ask questions; if a detail is missing, choose a sensible default."
[ -n "$size" ] && instruction="$instruction Target size: $size."
if [ "${#images[@]}" -gt 0 ]; then
  instruction="$instruction The attached image(s) are inputs; use them as described in the specification."
fi
instruction="$instruction After the image is saved, reply with ONLY its absolute filesystem path and nothing else.

--- specification ---
$prompt"

img_args=()
for f in "${images[@]}"; do
  [ -r "$f" ] || { echo "input image not readable: $f" >&2; exit 5; }
  img_args+=(-i "$f")
done

echo "[gen] codex exec (built-in image_gen, subscription) ..." >&2
# forkd is the outer sandbox, so bypass codex's own bwrap sandbox (nested user
# namespaces would fail) and its approval prompts. --skip-git-repo-check: the workspace
# is not a git repo.
set +e
codex_out="$("$codex_bin" exec \
  --skip-git-repo-check \
  --dangerously-bypass-approvals-and-sandbox \
  -C "$PWD" \
  "${img_args[@]}" \
  "$instruction" </dev/null 2>>"$marker.log")"
rc=$?
set -e
if [ $rc -ne 0 ]; then
  echo "codex exec failed (rc=$rc):" >&2
  tail -5 "$marker.log" >&2 2>/dev/null || true
  exit 6
fi

# Prefer the path codex printed on its last line; fall back to the newest PNG created
# under generated_images after our marker.
path="$(printf '%s\n' "$codex_out" | grep -aoE '/[^[:space:]]+\.png' | tail -1 || true)"
if [ -z "$path" ] || [ ! -f "$path" ]; then
  path="$(find "$gen_dir" -type f -name '*.png' -newer "$marker" 2>/dev/null | \
          xargs -r ls -1t 2>/dev/null | head -1 || true)"
fi
[ -n "$path" ] && [ -f "$path" ] || { echo "image was not produced (no new PNG under $gen_dir)" >&2; exit 7; }

cp -f "$path" "$out"
bytes="$(wc -c <"$out" | tr -d ' ')"
echo "[out] $out"
echo "[size] ${size:-auto}, ${bytes} bytes"
echo "[src] $path"
