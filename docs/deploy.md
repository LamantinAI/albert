# Deploy

Albert is a single binary. **Build on a dev box, ship the binary** — the target
(2 core / 2 GB) would struggle to compile the cozo/rocksdb/rig tree. No
cross-compilation is needed when the build host and target share arch + glibc
(here: both x86_64 Ubuntu 24.04, glibc 2.39); otherwise build a matching target
or a static musl binary.

> Prefer containers? `contrib/deploy/` has a self-contained **Docker** runbook
> (subscription mode; all state in one named volume) — see
> [`contrib/deploy/README.md`](../contrib/deploy/README.md). This page is the
> systemd-on-a-host path.

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
cargo build --release

# 2. stage: binary + albert.toml + soul.md/system.md + skills/ + config/ + .env + the
#    unit; set the real owner_chat in config/connectors/telegram/telegram.toml and the
#    account in config/connectors/calendar/calendar.toml (see configuration.md).

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
cargo build --release
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

- `config/connectors/telegram/telegram.toml` — `owner_chat` = your chat id
  (`@userinfobot`). The ACL starts empty and drops everyone (including you) until
  the owner is seeded; a wrong id locks you out (the safe failure).
- `config/connectors/calendar/calendar.toml` — your account: `login` (basic auth) or
  the Google `collection` + client; the secret stays in `.env`.
- `.env` — the actual secret values (LLM key, Telegram token, calendar credentials).
