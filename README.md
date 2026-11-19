# Albert

<p align="center">
  <img src="resources/albert.png" alt="Albert" width="240">
</p>

<p align="center"><b>An always-on AI assistant that grows with you — and stays fast doing it.</b></p>

`Albert` is built as an **assembly** of maturing LamantinAI substrates rather than one
model. Talk to him one-on-one or drop him into a group chat — he serves a person or a
whole team. Three things set the shape of it:

- **It grows with you.** Albert remembers with [kaeru](https://github.com/LamantinAI/kaeru),
  its memory — it keeps learning the more you (and your group) use it, far past what a
  single conversation could hold.
- **It stays fast.** It reaches for memory only when it needs to, and pulls just what
  matters — so replies stay quick no matter how much it knows.
- **It lives in an environment.** The [Octo](https://github.com/LamantinAI/octo) runtime
  wakes it on its own — reminders and routines just happen — and makes connecting a new
  app or service easy. So Albert is proactive, works across your channels, and picks up
  new skills without a rebuild.

You talk to it; it remembers, it reminds, and it keeps a verifiable scratchpad for
multi-step work.

## Quick start

### Start Albert

With Docker and the Compose plugin installed, create a deployment directory
and download the production Compose file:

```sh
curl -fsSLo docker-compose.yaml https://raw.githubusercontent.com/LamantinAI/albert/main/docker-compose.yaml

printf 'KAERU_MCP_AUTH_TOKEN=%s\n' "$(openssl rand -hex 32)" > .env
docker compose pull

mkdir ./albert
sudo chown 10001:10001 ./albert

docker compose up -d
docker compose logs -f
```

The image initializes `./albert` with configuration, prompts, connector manifests
and skills. Edit those files on the host; restart with `docker compose restart`
after changing startup configuration. Albert and Kaeru retain their state in Docker
volumes. `docker compose down` stops the services and keeps those volumes. The
production defaults already select the Kaeru MCP service, so the panel skips the
memory selection step. See the [Docker deployment guide](contrib/deploy/README.md).

### Web workspace

Open `http://127.0.0.1:9877/ui/` after starting Albert. Sign in with `owner` /
`owner`, replace the initial credentials, and connect a model. The embedded React
workspace supports private and temporary chats, model priorities, skills, users,
configuration and SQL approvals. Telegram is optional. See the
[web panel guide](docs/web-panel.md) for setup, permissions and frontend builds.

## Why Albert

Most assistants built on a coding-agent loop re-read a memory file and a wall of
instructions on **every** turn — so the more they know, the heavier, slower, and
pricier each reply gets. Albert is built the other way round:

- **Deliberate memory, not re-read every turn.** kaeru is a bi-temporal cognitive
  graph Albert *queries as a tool* — it recalls what's relevant on demand instead of
  stuffing a growing memory blob into every prompt. Context per turn stays small
  (lower latency, lower token cost), and memory keeps growing past what any context
  window could hold. It learns more without the prompt getting heavier.
- **Lean by design.** The system prompt is deliberately compact, and skills are a
  *catalog* (name + when-to-use) in the preamble — a skill's full instructions load
  only when it's applied. The model carries only what the turn actually needs.
- **It lives in an environment, not a chat loop.** Octo's event bus + supervised
  connectors make Albert proactive and multi-channel: reminders and routines fire on
  their own, connectors push events on their own cadence, and Albert can even restart
  its own parts to apply a config change. Not a request→response wrapper around a few
  functions.
- **Real hands.** A jailed file workspace (read/write/edit/glob/grep) with durable
  storage and file send/receive, plus a sandboxed script runner (forkd) for executable
  skills. It doesn't only talk — it fetches, transforms, and produces files.
- **Safe at the edge.** Untrusted chats are dropped *before* they reach cognition (an
  edge ACL — the prompt-injection boundary); scripts run as a lower-privileged user
  under a hardened systemd unit.

The net effect: a lighter, faster loop that stays sharp as it accumulates knowledge —
because knowledge lives in memory it recalls, not in a prompt it re-reads.

## What it does

- **Persistent memory (kaeru)** — recalls context on demand, remembers durable facts,
  reflects on a cadence; a real cognitive-graph substrate, not a bulk memory blob.
- **Reminders & calendar** — "remind me every hour to drink water" becomes a memory
  task + a recurring alarm; a one-off "remind me tomorrow at 2pm" becomes a real
  **calendar** event (a popup that syncs to your phone). When it fires Albert recalls
  the task and messages you; when you say it's done, it closes the task and cancels.
- **Proactive routines** — a self-scheduled memory-reflection pass Albert seeds on
  startup and runs silently on a cadence. Same machinery scales to more routines.
- **Files** — a jailed workspace (read/write/edit/list/glob/grep), durable **storage**
  (promote/checkout), and file exchange (send a file to you / receive one from you) —
  bytes move by reference, never pasted through the chat.
- **Skills** — a folder of `SKILL.md` recipes Albert sees as a catalog and applies on
  demand; **executable** skills bundle a script it runs in the forkd sandbox.
- **Scripts (forkd)** — run python3 / bash (with curl / wget) in a sandbox: jailed to
  the workspace, dropped privileges, clean environment, wall-clock timeout.
- **Loop scratchpad** — a super-operational, per-task working object the agent authors
  (`scratchpad_goal/step/mark/note`) and sees each turn, so multi-step work is
  **verifiable**: a task is done only when every step is `verified`.
- **Self-lifecycle control** — on an owner turn, Albert manages its own runtime: it can
  read and edit its own config/prompt files and then apply the change by restarting —
  reloading a connector's manifest, or its whole self (history + memory persist).
- **[Model pool](docs/model-pool.md)** — owner-controlled live selection, explicit
  capabilities and bounded fallback without automatically replaying tool actions.
- **Config-driven connectors** — Telegram (account-bound token login, `/chatinfo`,
  replies only when addressed in groups), a generic **CalDAV calendar** (Yandex/Fastmail/Nextcloud/iCloud/Google),
  a **scheduler**, **storage**, **forkd**, **search** (web search over several engines —
  DuckDuckGo now, Yandex next; the manifest sets the default and Albert can pick per
  query), and an optional **mail** organ (IMAP/SMTP, off by default) — each assembled
  from a small manifest.
- **Two brains, your call** — an **API key** (OpenRouter / any OpenAI-compatible
  endpoint; the default) or a **ChatGPT subscription** (via the Codex backend).
- **Choose where memory lives** — embedded kaeru or your own kaeru MCP server
  over HTTP/stdio. Albert creates its initiative on first startup if missing.
  See [memory configuration](docs/configuration.md#memory-backend-memory).
- **Config-as-data** — Albert-level settings in `albert.toml`; secrets in `.env`,
  named in the TOML by their env var (never the value); persona + a compact operating
  prompt in `soul.md` / `system.md`, held in RAM and **hot-reloaded by mtime**.

## PEMRR architecture

**PEMRR** is Albert's long-term architecture. Each layer has a distinct
responsibility.

- **Perception** receives messages, events, and observations from the environment.
- **Experience** interprets those observations in the context of the current task.
- **Memory** recalls relevant knowledge and records what should persist.
- **Reflection** reviews outcomes and identifies what to reuse or revise.
- **Reaction** takes action in the environment and communicates the result.

Today, connectors deliver observations, a single model-driven loop handles
Experience and Reflection, kaeru provides Memory, and Octo routes actions. Over
time, Albert should be able to build and reuse its own task structure from
scratchpad plans and verified outcomes. [Architecture guide](docs/architecture.md)

## Roadmap

The current release focuses on making Albert easier to set up and use.

### Next release — agent efficiency and speed

- **Session forks.** Start a separate task from an existing session and carry over
  its context. The original conversation remains available.
- **Lightweight browser use.** Reduce the startup time and resource cost of opening
  and interacting with web pages.
- **Faster web search.** Shorten the time from a query to results Albert can use.
- **Sandbox and workspace.** Simplify workspace layout and execution isolation so
  tools run reliably with less overhead.
- **Scratchpad task plans.** Record stages and dependencies, and prepare scripts for
  predictable steps that can run without another model call.

### Following release — coding mode

A dedicated mode with skills and tools for navigating and editing code, running
checks, and rendering and inspecting user interfaces. It will emphasize
syntax-aware navigation and concise command output.

Connector and model-provider coverage will continue to expand across releases.

## How it fits together

- **Octo** supervises connectors and routes typed events. Scheduling and runtime
  lifecycle controls run at this layer. [Octo](https://github.com/LamantinAI/octo)
- **The cogitator** runs the model's tool loop. It handles messages and scheduled
  events, calls tools, and produces replies. [rig](https://github.com/0xPlaygrounds/rig)
- **kaeru** provides a typed, bi-temporal memory graph. The cogitator writes and
  recalls relevant records through tools. [kaeru](https://github.com/LamantinAI/kaeru)

See the [architecture guide](docs/architecture.md) for the runtime and data flow.

## Source workspace

| Path | Responsibility |
| --- | --- |
| `albert-core/` | Cognition, sessions, and shared runtime state |
| `albert-server/` | Application assembly and the `albert` executable |
| `connectors/` | Albert's console, Telegram, and web adapters |
| `web/albert-panel/` | React workspace and generated TypeScript API contracts |

The web connector exports its OpenAPI schema from Utoipa. Browser assets are
generated at build time. Cargo fetches Octo and kaeru at pinned revisions.
See the [code map](docs/structure.md) for dependency and build boundaries.

## License

**Business Source License 1.1** — see [LICENSE](LICENSE), with a plain-language
summary in [LICENSING.md](LICENSING.md).

Albert is *source-available*: read it, modify it, run it in production, and embed
it as a component or tool inside your own systems and products — including what
you sell. The one thing you may not do is operate Albert itself as a hosted /
managed / SaaS platform for third parties. Each released version converts to the
**Apache License 2.0** four years after it ships (the Change Date in `LICENSE`).
Contributions are accepted under the [Contributor License Agreement](CLA.md).

Version v0.1.0 was released under the MIT License and remains available under it;
the move to BSL applies from the relicensing commit forward and is not
retroactive.
