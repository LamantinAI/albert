# Deploy

This guide installs Albert on a Linux host with systemd. Build the executable on a
compatible machine and ship it with its configuration, prompts and skills. The
build must match the target architecture and system library ABI.

For a container deployment, use the production Compose stack. It mounts Albert's
editable configuration and uses separate volumes for Albert and Kaeru. Both API
keys and ChatGPT subscriptions can be configured in the panel. [Docker Compose guide](../contrib/deploy/README.md)
For a standalone `docker run`, pass `--init` to reap child processes.

## Host prerequisites

Install the system libraries and tools used by your enabled connectors.

- **`curl` on `PATH`** — executable skills can invoke system tools through forkd.
- **`moli` on the service's `PATH`** — DuckDuckGo and page fetch share a Moli
  process. The Docker image includes Moli. For a native deployment install
  the matching release and ensure the service can reach the web. Albert starts
  the process during service startup when either connector uses it.
  [Moli releases](https://github.com/lexmount/moli/releases)

- **`ffmpeg` (with `ffprobe`)** — the transcribe connector cuts a long recording or a
  video on its pauses before uploading it (`sudo apt install ffmpeg`). Without it a
  file is uploaded whole: fine for short clips, but a long one can come back truncated.

## Layout on the target

Everything lives under `/opt/albert/`:

```
/opt/albert/
├── albert                 the release binary
├── albert.toml            Albert-level config
├── soul.md system.md      persona + instructions (edit live; hot-reloaded)
├── skills/                declarative + executable skills (<name>/SKILL.md)
├── config/                connector manifests (octo.toml + connectors/*)
├── .env                   secrets + ALBERT_CONFIG + KAERU_VAULT_PATH  (chmod 600)
├── state/                 runtime state (see below)
│   ├── history.db         SQLite chat transcript (survives restarts)
│   ├── history-backups/   consistent snapshots before internal schema upgrades
│   ├── scheduler.json     scheduler alarms
│   ├── workspace/         ephemeral file-tool jail (setgid 2770; agent <-> scripts)
│   └── artifacts/         durable storage-connector objects
└── kaeru/                 the kaeru memory vault (KAERU_VAULT_PATH)
```

The binary bakes no absolute paths that matter at runtime: `ALBERT_CONFIG` points
it at `albert.toml`, and every other path (prompts, skills, history, scheduler state,
workspace, the connector manifests) resolves from the config file's directory.

## `.env` (systemd `EnvironmentFile`)

Secrets only, plus the two location vars:

```sh
ALBERT_OPENAI_KEY=...            # LLM key
OCTO_TELEGRAM_TOKEN=...          # Telegram bot token
OCTO_YANDEX_APP_PASSWORD=...     # calendar (basic auth) — or the Google OAuth pair
# OCTO_MAIL_USER=... OCTO_MAIL_PASS=...   # only if you enable the off-by-default mail organ
ALBERT_CONFIG=/opt/albert/albert.toml
KAERU_VAULT_PATH=/opt/albert/kaeru
```

## systemd unit — `/etc/systemd/system/albert.service`

The canonical **hardened** unit lives in
[`contrib/deploy/albert.service`](../contrib/deploy/albert.service) (**L1** isolation:
confine the agent from the host). Albert runs as the unprivileged `albert` user
(`ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, capabilities bounded to
`CAP_SETUID CAP_SETGID` so forkd can still drop scripts to `albert-scripts`; `UMask=0007`
so workspace writes are group-shared for the handoff). Provision before first start:

```sh
# two system users: the service, and the lower-privileged one forkd drops scripts to
useradd --system --no-create-home --shell /usr/sbin/nologin albert
useradd --system --no-create-home --shell /usr/sbin/nologin albert-scripts
usermod -aG albert albert-scripts        # albert-scripts joins the workspace-sharing group

chown -R albert:albert /opt/albert/state /opt/albert/kaeru
chown albert:albert /opt/albert/config/connectors/telegram   # runtime ACL persists here
mkdir -p /opt/albert/state/workspace
chgrp albert /opt/albert/state/workspace
chmod 2770 /opt/albert/state/workspace   # setgid: agent + dropped script user share via group

cp contrib/deploy/albert.service /etc/systemd/system/albert.service
systemctl daemon-reload
```

Secrets stay in root-owned `0600` `/opt/albert/.env` — systemd reads it before
dropping privileges, so the service sees them but the files on disk stay root-only.

Why the two users + setgid workspace: that is the L2 boundary (agent ← scripts) and
its handoff surface. The full three-layer picture is in
[architecture.md](architecture.md#isolation-three-layers).

## First deploy

```sh
# 1. build the release binary on the dev box
npm --prefix web/albert-panel ci
npm --prefix web/albert-panel run build:embedded
cargo build --locked --release

# 2. Stage the host config with embedded memory, the binary, prompts, skills and manifests.
mkdir -p stage
cp albert.dev.toml stage/albert.toml
cp target/release/albert contrib/deploy/albert.service soul.md system.md stage/
cp -R config skills stage/
#    Create the deployment .env in stage/ and configure the calendar if used.
#    Connect Telegram and issue your personal login token in the panel after startup.

# 3. ship (tar over ssh keeps hidden files + perms; no rsync needed)
tar -C stage --exclude=albert.service -czf - . \
  | ssh <host> 'mkdir -p /opt/albert/kaeru && tar -C /opt/albert -xzf -'
scp stage/albert.service <host>:/etc/systemd/system/albert.service

# 4. provision users + the setgid workspace (above), then start
ssh <host> 'chmod 600 /opt/albert/.env && chmod +x /opt/albert/albert \
  && systemctl daemon-reload && systemctl enable --now albert.service'
```

## Operate

```sh
journalctl -u albert.service -f          # live logs
systemctl restart albert.service         # after editing albert.toml / a manifest
systemctl status albert.service
```

`soul.md` / `system.md` and the `skills/` bodies are hot / on-demand (edit + save on
the target, no restart). `albert.toml` and the connector manifests are read at
startup — restart after editing them. When the owner is chatting, Albert can do that
restart itself with its `restart` tool (`target: "process"` for the whole service,
`target: "<connector id>"` to reload one manifest).

## Update the binary

`install` preserves owner/mode; back up first so a bad build is one `mv` from rollback:

```sh
npm --prefix web/albert-panel ci
npm --prefix web/albert-panel run build:embedded
cargo build --locked --release
ts=$(date +%Y%m%d-%H%M%S)
ssh <host> "sudo cp /opt/albert/albert /opt/albert/albert.bak-$ts"
scp target/release/albert <host>:/tmp/albert.new
ssh <host> 'sudo install -o albert -g albert -m 0755 /tmp/albert.new /opt/albert/albert \
  && rm -f /tmp/albert.new && sudo systemctl restart albert.service \
  && systemctl is-active albert.service'
```

Ship prompt/skill/config changes the same way (they need no rebuild): `scp` them into
`/opt/albert/` (prompts + skills hot-reload; a manifest or `albert.toml` change wants a
restart).

## Per-target config (not committed)

The repo ships **placeholder** manifests. On the target set the real values:

- Telegram — save the BotFather credential inside the selected Telegram instance under **Settings → Connections**, then
  create a personal login token there and send `/login <token>` privately to the
  bot. `owner_chat` and allowlists no longer grant Albert permissions. Existing
  history is preserved; see [Telegram account access](telegram-accounts.md).
- `config/connectors/calendar/calendar.toml` — your account: `login` (basic auth) or
  the Google `collection` + client; the secret stays in `.env`.
- `.env` — the actual secret values (LLM key, Telegram token, calendar credentials).
