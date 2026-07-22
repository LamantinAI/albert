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
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/albert
COPY --from=build /albert /usr/local/bin/albert
# Config + persona + connector manifests + skills are baked in; all *runtime* state
# (tokens, kaeru vault, scheduler alarms) lives under /data via a mounted volume.
COPY contrib/deploy/albert.toml ./albert.toml
COPY soul.md system.md ./
COPY config ./config
COPY skills ./skills

ENV ALBERT_CONFIG=/opt/albert/albert.toml \
    KAERU_VAULT_PATH=/data/kaeru \
    RUST_LOG=albert=info,octo_rig=info,octo_connector_scheduler=info,octo_connector_telegram=info,octo_connector_caldav=info,octo_http_auth=info,octo_core=warn

ENTRYPOINT ["albert"]
