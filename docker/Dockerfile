# Albert — multi-stage build. Build stage compiles the release binary (cozo/rocksdb
# and the rig tree need clang + cmake); runtime stage is a slim Debian that just runs
# it. Target the server's arch when building: `--platform linux/amd64`.

# ---- build ----
FROM rust:1-bookworm AS build

# cozo/rocksdb: clang + libclang for bindgen, cmake for the C/C++ deps. libssl-dev
# covers a native-tls path if one is pulled in. (gcc/g++/make come with the image.)
RUN apt-get update && apt-get install -y --no-install-recommends \
        clang libclang-dev cmake pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .
# --locked: build exactly Cargo.lock (reproducible). The BuildKit cache mounts keep
# the cargo registry/git checkouts and the target dir warm across image builds, so
# only what actually changed recompiles: a source edit → just the albert crate
# (~1–2 min); an octo rev bump → the octo crates and up. Cold cache (first build,
# or after `docker builder prune`) is the old full ~15 min — cozo and rocksdb
# dominate; on a 2 GB host add swap first (see contrib/deploy/README.md). The binary is
# copied out of the mount because cache mounts are not part of the image.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
    && cp target/release/albert /albert

# ---- runtime ----
# python:3.12-slim-bookworm (not plain debian-slim) so forkd-run skill scripts
# have a python3 interpreter. bookworm → same glibc 2.36 as the rust:1-bookworm
# build stage, so the release binary links cleanly; the avito-price skill ships a
# vendored pylib compiled for exactly CPython 3.12 / x86_64, which this matches.
FROM python:3.12-slim-bookworm AS runtime

# ffmpeg (with ffprobe) backs the `transcribe` skill: it measures a recording,
# finds its silences and cuts it into chunks the dictation endpoint won't truncate.
# curl is both a fetch tool for skill scripts and how codex is installed below.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 ffmpeg curl \
    && rm -rf /var/lib/apt/lists/*

# The Codex CLI, so skills can drive it directly on the same ChatGPT subscription
# Albert already authenticates with. Shipped as one static musl binary (no node,
# no runtime deps) — pinned, because "latest" would make the image unreproducible.
ARG CODEX_VERSION=rust-v0.146.0
RUN curl -fsSL "https://github.com/openai/codex/releases/download/${CODEX_VERSION}/codex-x86_64-unknown-linux-musl.tar.gz" \
        | tar -xz -C /usr/local/bin \
    && mv /usr/local/bin/codex-x86_64-unknown-linux-musl /usr/local/bin/codex \
    && chmod 0755 /usr/local/bin/codex \
    && codex --version

WORKDIR /opt/albert
COPY --from=build /albert /usr/local/bin/albert
# Config + persona + connector manifests + skills are baked in; all *runtime* state
# (tokens, kaeru vault, scheduler alarms) lives under /data via a mounted volume.
COPY contrib/deploy/albert.toml ./albert.toml
COPY soul.md system.md ./
COPY config ./config
COPY skills ./skills

# CODEX_HOME on the volume so the CLI's own state survives rebuilds. Deliberately
# NOT /data itself: codex would then share Albert's auth.json and rewrite it on
# refresh, putting the running agent's token at the mercy of a CLI invocation. To
# let codex use the subscription, link the token in once:
#   docker compose -p deploy exec albert sh -c 'mkdir -p /data/codex && ln -sf /data/auth.json /data/codex/auth.json'
ENV ALBERT_CONFIG=/opt/albert/albert.toml \
    KAERU_VAULT_PATH=/data/kaeru \
    CODEX_HOME=/data/codex \
    RUST_LOG=albert=info,octo_rig=info,octo_connector_scheduler=info,octo_connector_telegram=info,octo_connector_caldav=info,octo_http_auth=info,octo_core=warn

ENTRYPOINT ["albert"]
