# Albert — Documentation

The "how it's built and why" reference for `Albert`. For a product overview and
quick start, see the top-level [`README.md`](../README.md); this folder is the
deeper reference.

## Contents

- **[architecture.md](architecture.md)** — the assembly: Octo as the Reaction
  skeleton, kaeru as deliberate memory (a tool, not a bulk blob), the cogitator's
  perceive → tool-loop → reply cycle, the three context tiers (chat / scratchpad /
  memory), reminders + proactive routines, and the config-driven connectors. Plus
  what is settled vs. future work.
- **[configuration.md](configuration.md)** — the config reference: `albert.toml`,
  the `.env` secrets, `soul.md` / `system.md`, and the Octo connector manifests
  (`config/octo.toml` + `config/connectors/*/*.toml`) for Telegram and the calendar.
- **[structure.md](structure.md)** — the code map: what each module owns and where
  a given concern lives.
- **[deploy.md](deploy.md)** — shipping Albert to a machine: build on a dev box,
  the `/opt/albert` layout, the systemd unit, and the per-target config.

## One-paragraph mental model

`Albert` is a single **Cogitator** running inside the [Octo](https://github.com/LamantinAI/octo)
event-driven runtime. Octo owns the substrate — a typed-envelope bus, connectors as
supervised long-lived tasks, a reflex/cognition split — and Albert plugs in as the
cognition layer: it perceives two event kinds (`chat.message`, `alarm.fired`) and runs
a `rig` native tool-loop. Its tools are its capabilities: **kaeru** for deliberate,
on-demand memory (recall / remember / reflect), the **scheduler** connector for
reminders and self-scheduled routines, and a **loop scratchpad** for verifiable
multi-step tasks. Three context tiers stay distinct — the chat transcript (a log,
Octo history), the scratchpad (super-operational task state), and kaeru (durable
memory, reached as a tool). Persona and instructions live in hot-reloaded files;
everything tunable lives in TOML; connectors (Telegram with an edge ACL, a generic
CalDAV calendar) are assembled from manifests. It is the working-first cut; deeper
cognition — the agent building and reusing **its own** task structure — is future
work, a self-built graph it authors (grown from the scratchpad), not a LangGraph-style
control-flow engine.
