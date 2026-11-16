#!/bin/sh
set -eu

# Preserve operator changes while adding defaults that are absent from the mount.
cp -rn /usr/share/albert/defaults/. /opt/albert/

exec albert "$@"
