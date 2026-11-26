# Albert — multi-stage build, both stages on Ubuntu 24.04.
#
# Use the same glibc ABI for the build and runtime. Ubuntu 24.04 also provides
# Python 3.12 for skill scripts.
#
# Build for the server's arch: `docker build --platform linux/amd64 …`.

# ---- build ----
FROM node:24.6.0-bookworm-slim AS node-tools
FROM ubuntu:24.04 AS build
COPY --from=node-tools /usr/local/ /usr/local/

ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
# Pin the toolchain (Ubuntu's apt rustc is too old for edition 2024).
ARG RUST_VERSION=1.97.0

# cozo/rocksdb uses clang, libclang and cmake. pkg-config and libssl-dev support
# TLS builds. git and curl bootstrap rustup and fetch Cargo dependencies.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential clang libclang-dev cmake pkg-config \
        libssl-dev \
        git curl ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && curl -sSf https://sh.rustup.rs | sh -s -- -y \
        --no-modify-path --profile minimal --default-toolchain ${RUST_VERSION}

WORKDIR /src
COPY . .
# --locked: build exactly Cargo.lock. BuildKit cache mounts keep the cargo registry/git
# and target dir warm across image builds, so only what changed recompiles. First build
# is the full ~15+ min (cozo/rocksdb dominate); on a 2 GB host add swap (see
# contrib/deploy/README.md). Copy the binary out of the build cache mount.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    npm --prefix web/albert-panel ci \
    && npm --prefix web/albert-panel run build:embedded \
    && cargo build --release --locked \
    && cp target/release/albert /albert

# ---- runtime ----
FROM ubuntu:24.04 AS runtime-base

ARG DEBIAN_FRONTEND=noninteractive
# Runtime libraries + tools:
#  - ca-certificates and libssl3: TLS.
#  - curl, wget, python3: what the forkd sandbox runs skill scripts with (python3 is
#    3.12 on Ubuntu 24.04, which the avito vendored parser targets).
#  - ffmpeg (with ffprobe): the transcribe connector measures a long recording (or a
#    video), finds its silences and cuts it into audio chunks the dictation endpoint
#    won't truncate; the `video` / `video-link` skills use it too.
#  - libfontconfig1 and fonts-liberation support Moli's text rendering.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 \
        curl wget python3 ffmpeg \
        libfontconfig1 libstdc++6 fonts-liberation \
    && rm -rf /var/lib/apt/lists/* \
    # Two users, mirroring the systemd deploy: `albert` runs the service; `albert-scripts`
    # is the lower-privileged user forkd drops skill scripts to. albert-scripts' PRIMARY
    # group is `albert` (setuid drops supplementary groups, so this is what lets it share
    # the group-readable workspace the agent writes).
    && groupadd --system --gid 10001 albert \
    && useradd --system --uid 10001 --no-create-home --gid albert --shell /usr/sbin/nologin albert \
    && useradd --system --uid 10002 --no-create-home --gid albert --shell /usr/sbin/nologin albert-scripts \
    # Runtime state lives on the /data volume; a fresh named volume inherits this owner.
    && mkdir -p /data && chown albert:albert /data

# Moli is shared by browser.fetch and DuckDuckGo.
ARG MOLI_VERSION=v1.1.27
ARG TARGETARCH
RUN case "${TARGETARCH}" in \
        amd64) moli_arch=x86_64; moli_sha=2ede138ca107f16313183be06ef137c0760eaeb83598570d7f4a4263595cd1a6 ;; \
        arm64) moli_arch=aarch64; moli_sha=f33ce375155f36beea672f9f45785a09ab8913c69955b166c7a479712b8456ad ;; \
        *) echo "Unsupported Moli architecture: ${TARGETARCH}" >&2; exit 1 ;; \
       esac \
    && curl -fsSL -o /tmp/moli.tar.gz \
        "https://github.com/lexmount/moli/releases/download/${MOLI_VERSION}/moli-${moli_arch}-unknown-linux-gnu.tar.gz" \
    && echo "${moli_sha}  /tmp/moli.tar.gz" | sha256sum -c - \
    && entry="$(tar -tzf /tmp/moli.tar.gz | awk '/(^|\/)moli$/ { print; exit }')" \
    && test -n "${entry}" \
    && tar -xOzf /tmp/moli.tar.gz "${entry}" > /usr/local/bin/moli \
    && chmod 0755 /usr/local/bin/moli \
    && rm /tmp/moli.tar.gz \
    && moli --version

# yt-dlp + deno, for the `video-link` skill: pull a video's audio from a link (YouTube,
# Shorts, VK, Rutube, …) so `transcribe` can read what is said in it. The standalone
# yt-dlp_linux build bundles its YouTube challenge solver; deno is the JavaScript runtime
# yt-dlp now needs to solve YouTube's challenges — without it formats go missing and
# downloads turn flaky. Both pinned and checksum-verified (a downloader that runs on
# every video link is worth not trusting blindly). yt-dlp breaks as YouTube changes, so
# expect to bump YTDLP_VERSION every few weeks: new tag + its line from SHA2-256SUMS.
ARG YTDLP_VERSION=2026.08.19
ARG YTDLP_SHA256=58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a
ARG DENO_VERSION=v2.9.7
ARG DENO_SHA256=c6527f24f4b16031d3ae4fa9f658d5f11534c8d84ce7dc8502420280919c3490
RUN curl -fsSL -o /usr/local/bin/yt-dlp \
        "https://github.com/yt-dlp/yt-dlp/releases/download/${YTDLP_VERSION}/yt-dlp_linux" \
    && echo "${YTDLP_SHA256}  /usr/local/bin/yt-dlp" | sha256sum -c - \
    && chmod 0755 /usr/local/bin/yt-dlp \
    && curl -fsSL -o /tmp/deno.zip \
        "https://github.com/denoland/deno/releases/download/${DENO_VERSION}/deno-x86_64-unknown-linux-gnu.zip" \
    && echo "${DENO_SHA256}  /tmp/deno.zip" | sha256sum -c - \
    && python3 -c 'import zipfile; zipfile.ZipFile("/tmp/deno.zip").extractall("/usr/local/bin")' \
    && chmod 0755 /usr/local/bin/deno \
    && rm /tmp/deno.zip \
    && yt-dlp --version \
    && deno --version

WORKDIR /opt/albert
# The entrypoint seeds a writable deployment mount without replacing edited files.
COPY soul.md system.md /usr/share/albert/defaults/
COPY config /usr/share/albert/defaults/config
COPY skills /usr/share/albert/defaults/skills
COPY --chmod=0755 docker/entrypoint.sh /usr/local/bin/albert-entrypoint
RUN mkdir -p /data/state \
    && chown albert:albert /data/state /opt/albert \
    && ln -s /data/state /usr/share/albert/defaults/state
ENV HOME=/data
USER albert
ENTRYPOINT ["albert-entrypoint"]

# Development mounts a binary built on the host. Both stages share system libraries.
FROM runtime-base AS development
COPY albert.dev.toml /usr/share/albert/defaults/albert.toml

FROM runtime-base AS runtime
COPY albert.toml /usr/share/albert/defaults/albert.toml
COPY --from=build /albert /usr/local/bin/albert
