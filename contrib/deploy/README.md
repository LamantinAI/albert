# Deploy Albert with Docker Compose

The root [production Compose file](../../docker-compose.yaml) runs the published
Albert and Kaeru MCP images. The [development Compose file](../../docker-compose.dev.yaml)
runs a binary built on your machine and also defines the isolated [e2e profile](../../docs/local-testing.md#local-subscription-onboarding-tests).

## Production

Follow the [quick start](../../README.md#quick-start) to prepare the directory,
set `KAERU_MCP_AUTH_TOKEN` in `.env`, and start the stack. The same token
authenticates Albert to Kaeru over a private Compose network. Open the web
workspace on port 9877, rotate the initial owner credentials, and connect a
model in the panel.

The image seeds missing configuration, prompts, skills and manifests under
`./albert`. Subsequent starts preserve existing files. The shipped `albert.toml`
explicitly selects Kaeru MCP. A workspace that supplies its own valid
`[memory]` backend skips memory selection in the panel. Missing configuration
on a fresh installation triggers the memory choice after model setup. An
initialization error can be retried from the panel.

Albert's history, subscription credentials, workspace and alarms live in
`albert-data`. Kaeru retains its data and configuration in its own volumes.
Keep the Compose project name and these volumes when upgrading. If updating an
installation downloaded as `compose.yml`, replace that file with the new
production Compose content, or invoke `-f docker-compose.yaml` explicitly.
Existing files in `./albert` continue to take precedence over packaged defaults.
For an older source-build deployment using `albert.local.toml`, put the live
file at `./albert/albert.toml` and carry over custom manifests and prompts before
starting with the new Compose file.

```sh
docker compose logs -f
docker compose restart albert
docker compose pull
docker compose up -d
docker compose down
```

Config and model paths, writable storage paths and the logging filter are set
by the Compose file. Prompts and skills can be edited in `./albert` while the
service is running. Restart after changing startup config or connector manifests.
Treat `.env`, auth stores and backups as private.

## Development

From a compatible Linux host with the checkout and frontend dependencies:

```sh
npm --prefix web/albert-panel ci
npm --prefix web/albert-panel run build:embedded
cargo build --locked
docker compose -f docker-compose.dev.yaml up -d --build
```

The development image seeds the deployment volume from `albert.dev.toml`. It
binds the panel to host loopback and mounts `target/debug/albert` read-only. Set
`ALBERT_DEV_BUILD_PROXY` if your Docker builder needs a download proxy. Set
`ALBERT_DEV_BINARY` for a different local build, `ALBERT_DEV_PORT` for a different
host port, and `ALBERT_DEV_ENV_FILE` for the local environment file. If the
development binary is replaced by Cargo, recreate the dev container to remount
the new file. The dev deployment and data use their own named volumes.

For the isolated browser suite, build with `--features local-e2e` and use
`python3 docker/local-e2e/run.py --binary target/debug/albert`. The runner
starts the test profile with its own project and volumes. The test agent only
reaches its mock provider through an internal network.

For a custom production build, use the root [Dockerfile](../../docker/Dockerfile)
and set the `albert` service image to the resulting tag. The final image embeds
the release binary and UI built from the lockfiles.
