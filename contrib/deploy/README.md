# Deploy Albert in Docker (ChatGPT-subscription mode)

A self-contained Docker deployment that runs Albert on the **Codex subscription** —
no API key, no `codex` CLI on the server. All runtime state (tokens, kaeru memory,
history, alarms) lives in one named volume, so restarts and rebuilds lose nothing.

> **Run these on a machine that can reach the server** (your laptop over the VPN, or
> the server itself). The server here (`185.130.225.215`) firewalls SSH to whitelisted
> source IPs, so a sandboxed/CI host generally cannot reach it — hence this runbook is
> executed by you, not by the agent.

Files: [`Dockerfile`](Dockerfile) · [`docker-compose.yml`](docker-compose.yml) ·
[`albert.toml`](albert.toml) (subscription config, container paths) ·
[`.env.example`](.env.example).

---

## 0. Prerequisites on the server

- Docker Engine + the compose plugin: `docker --version && docker compose version`.
  (Ubuntu 24.04: `curl -fsSL https://get.docker.com | sh`.)
- The Albert source on the server (the build needs it). Either:
  ```sh
  git clone https://github.com/LamantinAI/albert.git && cd albert
  # …or from your machine:  rsync -az --exclude target --exclude .git ./albert/ root@SERVER:/root/albert/
  ```

## 1. Give the build enough memory (2 GB is tight)

cozo/rocksdb are memory-hungry to compile. On a 2 GB box add swap first, or the build
OOM-kills:

```sh
fallocate -l 4G /swapfile && chmod 600 /swapfile && mkswap /swapfile && swapon /swapfile
echo '/swapfile none swap sw 0 0' >> /etc/fstab      # persist across reboots
```

(Alternatively build the image on a bigger machine and ship it — see “Build elsewhere”.)

## 2. Configure + build

```sh
cd albert                       # repo root, where docker-compose.yml lives
cp contrib/deploy/.env.example contrib/deploy/.env   # edit later for Telegram
cp contrib/deploy/albert.toml ./albert.local.toml    # your LIVE config (git-ignored) — edit this
docker compose build            # first build ~10–25 min (cozo/rocksdb dominate)
```

`./albert.local.toml` is your own copy, bind-mounted read-write; the tracked
`contrib/deploy/albert.toml` stays a pristine default. Create it before the first `up` —
Docker would otherwise put an empty directory in its place. It is NOT baked into the image
on purpose: the `self-config` skill edits this file at runtime, and a baked copy would sit
in the image's ephemeral layer, so those edits would report success and then vanish on the
next `up -d`.

Rebuilds are fast: the Dockerfile mounts BuildKit caches for the cargo registry and
the target dir, so after the first build only what changed recompiles (a source edit
→ ~1–2 min). `docker builder prune` clears those caches back to a cold build.

Model/timezone live in `albert.toml`. Edit your live `./albert.local.toml` (bind-mounted, see
`docker-compose.yml`) and `docker compose restart albert` — no rebuild. Nothing about a
model, a connector or a prompt should ever cost an image rebuild; if it does, a mount is
missing. Rebuilds are for Rust code and the Dockerfile.

## 3. Sign in with the ChatGPT subscription (once)

```sh
docker compose run --rm albert login
```

It prints an `auth.openai.com/oauth/authorize…` URL and waits. Because the server is
headless, use the **paste fallback**:

1. Open that URL in a browser **on your laptop** and sign in to ChatGPT.
2. The browser redirects to `http://localhost:1455/auth/callback?code=…&state=…`
   (it will show “can’t connect” — that’s fine).
3. Copy that whole redirect URL from the address bar and **paste it into the
   `albert login` prompt**, press Enter.

Tokens are written to `/data/auth.json` inside the `albert-data` volume and refreshed
in place before they expire. (Already have `~/.codex/auth.json`? Instead of logging in
you can drop it into the volume: `docker run --rm -v albert_albert-data:/data -v
$PWD:/host alpine cp /host/auth.json /data/auth.json`.)

## 4. Verify the subscription works (console, interactive)

```sh
docker compose run --rm albert
```

Startup should log `auth=subscription … base_url=https://chatgpt.com/backend-api/codex`
and `subscription auth loaded plan=…`. Type a message (e.g. `say one word: working`)
— a reply means the Codex subscription is live on the server. `Ctrl-C` to exit.

## 5. Run as a service

A background service needs a **channel** (console is interactive-only). Set up Telegram:

1. In `config/connectors/telegram/telegram.toml` set `owner_chat` = your numeric chat id
   (from `@userinfobot`). A placeholder locks you out — the ACL drops everyone until the
   owner is seeded.
2. Put the bot token in `contrib/deploy/.env`: `OCTO_TELEGRAM_TOKEN=…` (from `@BotFather`).
3. These are bind-mounted (manifests, `.env`, `./albert.local.toml`) — no rebuild needed, just:

```sh
docker compose up -d
docker compose logs -f
```

Without a Telegram token the service starts a console channel that has no interactive
input under `up -d` — fine for a smoke test, useless as a daemon. So set Telegram (or
another connector) for a real deployment.

## Operate

```sh
docker compose logs -f           # live logs
docker compose restart           # after editing albert.toml / connector manifests
docker compose pull  # n/a (local image); to update: git pull && docker compose build && docker compose up -d
docker compose down              # stop (keeps the data volume)
```

`soul.md` / `system.md` are hot-reloaded **only if bind-mounted** (see compose); baked
copies need a rebuild. `albert.toml` (your git-ignored `./albert.local.toml`) and connector
manifests are bind-mounted and read at startup — edit, then restart. The self-config skill
writes to that same mounted file, so its edits persist across restarts too.

> Known limitation (docker): the live config is a single operator-owned mount today. The
> cleaner shape — seed a default config TREE onto the `/data` volume on first start if it's
> empty (the Postgres `initdb` pattern), with every config path pointed at the volume — is
> queued for the platform/per-tenant work, not done yet.

## Build elsewhere (skip building on the 2 GB box)

Build the **linux/amd64** image on a capable machine, ship it, load it:

```sh
# on a build host (x86_64, or via emulation):
docker build --platform linux/amd64 -f docker/Dockerfile -t albert:latest .
docker save albert:latest | zstd | ssh root@SERVER 'zstd -d | docker load'
# on the server: docker compose up -d   (compose reuses the loaded albert:latest image)
```

## Notes

- **Tokens = password-equivalent.** They live only in the `albert-data` volume
  (`auth.json`, written `0600`). Don’t commit `contrib/deploy/.env`; it’s gitignored.
- **ToS.** Subscription mode uses the first-party Codex OAuth client + the
  `backend-api/codex` endpoint — outside OpenAI’s officially-supported third-party use.
  Opt-in, your subscription, your call.
