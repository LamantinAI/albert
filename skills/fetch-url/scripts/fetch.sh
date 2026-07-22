#!/usr/bin/env bash
# fetch-url skill: fetch a URL's body (or headers) from the forkd sandbox.
# Usage: fetch.sh <url> [headers]
set -euo pipefail
url="${1:?usage: fetch.sh <url> [headers]}"
mode="${2:-body}"
if [ "$mode" = "headers" ]; then
  exec curl -sSLI --max-time 20 -A 'albert/1.0' "$url"
fi
exec curl -sSL --max-time 20 -A 'albert/1.0' "$url"
