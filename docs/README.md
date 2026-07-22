# Albert — Documentation

The "how it's built and why" reference for `Albert`. For a product overview and
quick start, see the top-level [`README.md`](../README.md); this folder is the
deeper reference.

## Contents

- **[architecture.md](architecture.md)** — the assembly: Octo as the Reaction
  skeleton, kaeru as deliberate memory (a tool, not a bulk blob), the cogitator's
  perceive → tool-loop → reply cycle, the three context tiers (chat / scratchpad /
  memory), the file workspace + storage + file exchange, skills, the sandboxed forkd
  script runner and its three isolation layers, owner-gated self-restart, reminders +
  proactive routines, and the config-driven connectors. Plus what is settled vs. future.
- **[configuration.md](configuration.md)** — the config reference: `albert.toml`
  (auth, history, skills, workspace, clouds), the `.env` secrets, `soul.md` /
  `system.md`, and the Octo connector manifests (Telegram, calendar, storage, forkd) —
  ending in a complete, copyable working config.
- **[structure.md](structure.md)** — the code map: what each module owns and where
  a given concern lives.
- **[deploy.md](deploy.md)** — shipping Albert to a machine: build on a dev box,
  the `/opt/albert` layout, the hardened systemd unit + user/workspace provisioning,
  the Docker alternative, and the per-target config.

## One-paragraph mental model

`Albert` is a single **Cogitator** running inside the [Octo](https://github.com/LamantinAI/octo)
event-driven runtime. Octo owns the substrate — a typed-envelope bus, connectors as
supervised long-lived tasks, a reflex/cognition split, a control-plane — and Albert
plugs in as the cognition layer: it perceives two event kinds (`chat.message`,
`alarm.fired`) and runs a `rig` native tool-loop. **Its tools are its capabilities:**
**kaeru** for deliberate, on-demand memory (recalled as a tool, not re-read every
turn); the **scheduler** + **calendar** for reminders and self-scheduled routines; a
**file workspace** (read/write/edit/glob/grep) with durable **storage** and file
exchange (`send_file`); **skills** (a catalog in the preamble, bodies loaded on
demand); a sandboxed **forkd** script runner for executable skills; and — on an owner
turn — a **restart** tool to apply its own config changes. Three context tiers stay
distinct — the chat transcript (a log, Octo history, SQLite-persisted), the scratchpad
(super-operational task state), and kaeru (durable memory). Persona and a deliberately
**compact** system prompt live in hot-reloaded files; everything tunable lives in TOML;
connectors are assembled from manifests. It is the working-first cut; deeper cognition —
the agent building and reusing **its own** task structure — is future work, a self-built
graph it authors (grown from the scratchpad), not a LangGraph-style control-flow engine.
