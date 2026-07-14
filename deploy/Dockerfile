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
# --locked: build exactly Cargo.lock (reproducible). This is the slow step — cozo and
# rocksdb dominate; on a 2 GB host add swap first (see deploy/README.md).
RUN cargo build --release --locked

# ---- runtime ----
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/albert
COPY --from=build /src/target/release/albert /usr/local/bin/albert
# Config + persona + connector manifests + skills are baked in; all *runtime* state
# (tokens, kaeru vault, scheduler alarms) lives under /data via a mounted volume.
COPY deploy/albert.toml ./albert.toml
COPY soul.md system.md ./
COPY config ./config
COPY skills ./skills

ENV ALBERT_CONFIG=/opt/albert/albert.toml \
    KAERU_VAULT_PATH=/data/kaeru \
    RUST_LOG=albert=info,octo_rig=info,octo_connector_scheduler=info,octo_connector_telegram=info,octo_core=warn

ENTRYPOINT ["albert"]
