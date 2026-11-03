# Local subscription onboarding tests

The isolated Docker harness runs the real Albert binary and embedded panel with
synthetic OAuth and Codex services. It never reads deployment environment files,
mounts a host subscription store, or connects real message channels. Containers
share an internal network. A small fixed-upstream gateway exposes the panel and
fixture on loopback without giving the agent outbound internet. Each run uses
its own Compose project and fresh state volume.

Build a development binary with the opt-in `local-e2e` feature, then run:

```sh
cargo build --locked --features local-e2e
python3 docker/local-e2e/run.py --binary target/debug/albert
```

The feature enables startup-only fixture endpoints. It is disabled in production
builds; HTTP clients cannot choose token endpoints or credential paths. Use only
synthetic accounts with a fixture-enabled binary. Build the binary on a compatible
Linux system; the harness runtime uses Ubuntu 24.04 and its system libraries.

The runner creates its build context, frontend test copy, reports and logs in a
temporary directory outside the repository. Install the locked frontend packages
and Playwright Chromium first. `ALBERT_BROWSER_PATH` may select an already installed
Chromium executable. An optional `--frontend` points at a prepared dependency and
source copy outside Git; it must match the current source tree.

The browser suite covers first sign-in, mandatory owner password change, guided
authorization and callback paste, persisted subscription credentials, the account
model catalog, explicit model selection and a streamed response. Before selection,
no model is added to the pool. Catalog retries reuse the connected subscription. Additional cases cover invalid callbacks,
cancelled attempts, owner permissions, CSRF, refresh and restart persistence.
Passing the fixture suite does not establish eligibility with the live provider.

Normal completion and failures stop only that run's containers and remove its
state volume. `--keep` retains the isolated stack for manual inspection;
`--skip-browser --keep` starts a manual preview and explicitly reports that browser
tests were not run. The printed URLs and private `run.json` identify that stack.
Clean up a retained run without reconstructing its Compose variables:

```sh
python3 docker/local-e2e/run.py --cleanup /path/to/run-artifacts
```

The cleanup command targets only that run's generated project and volume. It
refuses nonlocal Docker contexts. Do not use global Docker prune commands.

Changes to an unpublished Octo credential component can be tested with a private
external Cargo patch configuration. Keep local paths and generated lockfile
overrides out of commits; publish and pin the dependency before shipping Albert.
For that unpublished development case, use `cargo --config /path/to/local-patch.toml
build --features local-e2e` instead of the locked build above. That override is
local tooling configuration, not part of the deployment configuration.
