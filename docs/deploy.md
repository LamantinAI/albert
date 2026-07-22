# Deploy

Albert is a single binary. **Build on a dev box, ship the binary** — the target
(2 core / 2 GB) would struggle to compile the cozo/rocksdb/rig tree. No
cross-compilation is needed when the build host and target share arch + glibc
(here: both x86_64 Ubuntu 24.04, glibc 2.39); otherwise build a matching target
or a static musl binary.

## Layout on the target

Everything lives under `/opt/albert/`:

```
/opt/albert/
├── albert                 the release binary
├── albert.toml            Albert-level config
├── soul.md system.md      persona + instructions (edit live; hot-reloaded)
├── config/                connector manifests (octo.toml + connectors/*)
├── .env                   secrets + ALBERT_CONFIG + KAERU_VAULT_PATH  (chmod 600)
├── state/                 scheduler alarms (runtime)
└── kaeru/                 the kaeru memory vault (KAERU_VAULT_PATH)
```

The binary bakes no absolute paths that matter at runtime: `ALBERT_CONFIG` points
it at `albert.toml`, and every other path (prompts, scheduler state, the connector
manifest) resolves from the config file's directory.

## `.env` (systemd `EnvironmentFile`)

Secrets only, plus two locations:

```sh
ALBERT_OPENAI_KEY=...            # LLM key
OCTO_TELEGRAM_TOKEN=...          # Telegram bot token
OCTO_YANDEX_APP_PASSWORD=...     # Yandex APP password (calendar)
ALBERT_CONFIG=/opt/albert/albert.toml
KAERU_VAULT_PATH=/opt/albert/kaeru
```

## systemd unit — `/etc/systemd/system/albert.service`

The canonical **hardened** unit lives in
[`contrib/deploy/albert.service`](../contrib/deploy/albert.service) — Albert runs as
the unprivileged `albert` user (`ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`,
capabilities bounded to `CAP_SETUID CAP_SETGID` so forkd can still drop scripts to
`albert-scripts`). Provision before first start:

```sh
useradd --system --no-create-home --shell /usr/sbin/nologin albert
useradd --system --no-create-home --shell /usr/sbin/nologin albert-scripts
usermod -aG albert albert-scripts        # workspace handoff group
chown -R albert:albert /opt/albert/state /opt/albert/kaeru
chown albert:albert /opt/albert/config/connectors/telegram   # runtime ACL persists here
chmod 2770 /opt/albert/state/workspace   # setgid: agent + scripts share via group
cp contrib/deploy/albert.service /etc/systemd/system/albert.service
systemctl daemon-reload
```

Secrets stay in root-owned `0600` `/opt/albert/.env` — systemd reads it before
dropping privileges, so the service sees them but the files on disk stay root-only.

## First deploy

```sh
# 1. build the release binary on the dev box
cargo build --release

# 2. stage: binary + config + prompts + .env + the unit; set the real owner_chat
#    in config/connectors/telegram/telegram.toml, and the Yandex login in
#    config/connectors/calendar/calendar.toml.

# 3. ship (tar over ssh keeps hidden files + perms; no rsync needed)
tar -C stage --exclude=albert.service -czf - . \
  | ssh <host> 'mkdir -p /opt/albert/kaeru && tar -C /opt/albert -xzf -'
scp stage/albert.service <host>:/etc/systemd/system/albert.service

# 4. start
ssh <host> 'chmod 600 /opt/albert/.env && chmod +x /opt/albert/albert \
  && systemctl daemon-reload && systemctl enable --now albert.service'
```

## Operate

```sh
journalctl -u albert.service -f          # live logs
systemctl restart albert.service         # after editing config
systemctl status albert.service
```

`soul.md` / `system.md` are hot-reloaded (edit + save on the target, no restart).
`albert.toml` and the connector manifests are read at startup — restart after
editing them.

## Update the binary

```sh
cargo build --release
scp target/release/albert <host>:/opt/albert/albert.new
ssh <host> 'mv /opt/albert/albert.new /opt/albert/albert && systemctl restart albert.service'
```

## Per-target config (not committed)

The repo ships **placeholder** manifests. On the target set the real values:

- `config/connectors/telegram/telegram.toml` — `owner_chat` = your chat id
  (`@userinfobot`). The ACL starts empty and drops everyone (including you) until
  the owner is seeded; a wrong id locks you out (the safe failure).
- `config/connectors/calendar/calendar.toml` — `login` = your Yandex address; the
  app password stays in `.env` under `OCTO_YANDEX_APP_PASSWORD`.
