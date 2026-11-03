"""Seed an isolated container volume; never read host configuration."""

import os
from pathlib import Path

os.umask(0o077)
root = Path("/data")
config = root / "albert.toml"
if not config.exists():
    config.write_text(
        "connectors_manifest = 'empty-connectors.toml'\n"
        "[subscription]\nauth_json = 'auth.json'\n"
        "[web]\nbind = '0.0.0.0:9877'\n"
        "[history]\nbackend = 'sqlite:state/history.db'\n"
        "[code]\nworkspace = 'workspace'\n"
        "[skills]\ndir = 'skills'\n"
    )
    (root / "empty-connectors.toml").write_text("[connectors]\ndir='connectors'\n")
    for directory in ("state", "connectors", "skills", "workspace"):
        (root / directory).mkdir(exist_ok=True)
os.execv("/usr/local/bin/albert", ["albert"])
