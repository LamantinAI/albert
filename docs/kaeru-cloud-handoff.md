# Design handoff — wiring kaeru cloud into Albert

Status: **applied 2026-07-14 (variant (b)).** Albert now reads `[clouds.*]` from
its config, builds a `CloudRegistry`, and installs the cloud tools only when a
cloud is configured — otherwise it stays local-only. This page is kept as the
design record. Applied in: `config.rs` (`CloudEndpoint` + `[clouds]` parse),
`main.rs` (registry + `with_clouds`), `cogitator.rs` (`install` vs
`install_with_cloud` on `clouds.is_empty()`), `albert.toml` (commented example).

## What changed upstream (kaeru-rig)

`kaeru-rig` used to be local-only (embedded `kaeru-core`, no network). It now
**optionally** reaches one or more `kaeru-cloud` endpoints in-process — the same
sharing/recall surface the `kaeru-mcp` daemon exposes, mirrored into rig (its own
`cloud_client.rs`; deliberate duplication, no shared crate).

New public API Albert consumes:

```rust
use kaeru_rig::{CloudClient, CloudRegistry, KaeruMemory};

// Build a client per endpoint (base_url + bearer token).
CloudClient::new(base_url: String, token: String) -> CloudClient

// Named multi-cloud, plus an optional default name.
CloudRegistry::new(clients: HashMap<String, CloudClient>, default: Option<String>) -> CloudRegistry

// Memory scoped to an initiative AND wired to the clouds.
KaeruMemory::with_clouds(store: Arc<Store>, initiative: impl Into<String>, clouds: CloudRegistry) -> KaeruMemory

// Install the full local surface + the cloud tools in one call.
mem.install_with_cloud(builder) -> builder   // vs the local-only `install(builder)`
```

The cloud tools added by `install_with_cloud`: `kaeru_policy`, `kaeru_share`,
`kaeru_cloud_recall`, `kaeru_pull`, `kaeru_link_cloud`, `kaeru_cloud_links`,
`kaeru_sync_review`. Each network tool takes an optional `cloud` arg (a name) —
omit it to hit the registry default.

## The boundary

Albert owns the **config**; kaeru owns the **client + tools + constructor**.
Albert reads endpoints/tokens from its own config and hands kaeru a built
`CloudRegistry`. Albert never touches `reqwest` or the cloud protocol.

This keeps Albert's existing rule intact: config files name secrets by their
**env-var name**, never the value (see `docs/configuration.md`).

## Four wiring points

1. **Bump the kaeru rev.** Albert pins `kaeru-core` / `kaeru-rig` to an old rev
   (`adff3db`, before the cloud work). Bump both to the rev that carries
   `with_clouds` / `install_with_cloud`.

2. **`albert.toml` — a `[clouds.*]` schema.** One section per cloud; endpoint URL
   + the env-var **name** of its bearer token. Optional `default`.

   ```toml
   [clouds.family]
   url = "https://cloud.family.example"
   token_env = "ALBERT_CLOUD_FAMILY_TOKEN"

   [clouds.work]
   url = "https://kaeru.work.example"
   token_env = "ALBERT_CLOUD_WORK_TOKEN"

   [clouds]
   default = "family"   # optional; a single cloud is the implicit default
   ```

   Token *values* live in `.env` (`ALBERT_CLOUD_FAMILY_TOKEN=...`), like every
   other secret.

3. **`config.rs` — parse it.** Add a `CloudEndpoint { url: String, token_env:
   String }` and carry `clouds: HashMap<String, CloudEndpoint>` +
   `clouds_default: Option<String>` on the Albert config. (An absent `[clouds.*]`
   → empty map → Albert stays local-only, no behaviour change.)

4. **`main.rs` — build the registry, swap the constructor** (currently ~L82-83
   `KaeruMemory::with_initiative(Arc::new(store), "albert")`):

   ```rust
   let clients = config.clouds.iter().map(|(name, ep)| {
       let token = std::env::var(&ep.token_env).unwrap_or_default();
       (name.clone(), CloudClient::new(ep.url.clone(), token))
   }).collect();
   let registry = CloudRegistry::new(clients, config.clouds_default.clone());
   let memory = KaeruMemory::with_clouds(Arc::new(store), "albert", registry);
   ```

5. **`cogitator.rs` — swap `install`** (currently ~L336
   `m.install(base).tool(dispatch)…`): use `install_with_cloud(base)` instead.
   The rest of the tool chain is unchanged.

   > Note: rig's builder is type-state, so the cloud tools can't be toggled at
   > runtime *inside* one `install`. Two choices: (a) always call
   > `install_with_cloud` — the cloud tools load even with an empty registry and
   > just answer "cloud not configured" until one is wired; or (b) branch on
   > `config.clouds.is_empty()` and pick `install` vs `install_with_cloud` (two
   > code paths, since the returned builder types differ — duplicate the tail
   > chain, or factor the `.tool(dispatch)…` tail into a small helper called from
   > both). (a) is simplest; (b) keeps the model's tool list clean when no cloud
   > is set. Albert's call.

## Design notes (why it looks like this)

- **Async model.** Only the cloud tools are natively async (real network I/O,
  `.await` the client). The local tools stay sync-on-`spawn_blocking` — Cozo is a
  synchronous embedded DB with no async API, so "make it async" there would mean
  blocking the executor. This matters for Albert specifically: the kaeru runtime
  is shared with the Octo bus / Telegram / SSE, and a blocking Cozo call on an
  executor thread would stall them. The cloud tools keep store touches on
  `spawn_blocking` for the same reason.

- **Two gates, fail-safe.** `share` requires the initiative's policy to permit it
  (`kaeru_policy … team`) AND the node to clear the strict pre-share secret guard
  (`guard::scan_public`). A refusal is a message, never a silent push. `force`
  overrides the guard.

- **Multi-cloud.** A node/soft-link remembers which cloud it belongs to; tools
  route by the `cloud` arg or the default. `kaeru_sync_review` and `kaeru_policy`
  are purely local (no network) and work whether or not a cloud is reachable.

- **Graceful when unconfigured.** With an empty registry, network tools return
  `{"error": "cloud not configured …"}` — no panic, no hang.

## Non-goals for this handoff

- No capture-and-share (`visibility: shared` on `remember`/`cite`) in rig yet —
  the mcp has it; rig's capture tools stay local-only for now.
- No change to Albert's local memory behaviour when `[clouds.*]` is absent.
