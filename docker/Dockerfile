# Albert — multi-stage build, both stages on Ubuntu 24.04.
#
# Why one base for both: the release binary dynamically links the system libcurl
# (the search connector's DuckDuckGo backend fetches through it, and DDG's anti-bot
# keys on that libcurl's TLS fingerprint — see connectors/search docs). Building and
# running on the same Ubuntu 24.04 means the same libcurl 8.5 / libcurl.so.4 at both
# ends, no cross-distro skew. Ubuntu 24.04 also ships python3.12 natively — what the
# forkd skill scripts (e.g. avito's vendored parser) expect.
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

# cozo/rocksdb: clang + libclang for bindgen, cmake for the C/C++ deps. pkg-config +
# libssl-dev for TLS. libcurl4-openssl-dev so the search connector's `curl` crate links
# the SYSTEM libcurl — WITHOUT it curl-sys silently vendors its own build, whose TLS
# handshake DuckDuckGo drops. git + curl bootstrap rustup.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential clang libclang-dev cmake pkg-config \
        libssl-dev libcurl4-openssl-dev \
        git curl ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && curl -sSf https://sh.rustup.rs | sh -s -- -y \
        --no-modify-path --profile minimal --default-toolchain ${RUST_VERSION}

WORKDIR /src
COPY . .
# --locked: build exactly Cargo.lock. BuildKit cache mounts keep the cargo registry/git
# and target dir warm across image builds, so only what changed recompiles. First build
# is the full ~15+ min (cozo/rocksdb dominate); on a 2 GB host add swap (see
# contrib/deploy/README.md). The binary is copied out because cache mounts aren't in the
# image. `cargo clean -p curl-sys` guards the trap where a build-script decision cached
# from before libcurl-dev was present would keep a vendored libcurl.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo clean -p curl-sys --release 2>/dev/null; \
    npm --prefix web/albert-panel ci \
    && npm --prefix web/albert-panel run build:embedded \
    && cargo build --release --locked \
    && cp target/release/albert /albert

# ---- runtime ----
FROM ubuntu:24.04 AS runtime-base

ARG DEBIAN_FRONTEND=noninteractive
# Runtime libraries + tools:
#  - ca-certificates, libssl3, libcurl4: TLS + the libcurl the binary links (and the
#    search backend uses).
#  - curl, wget, python3: what the forkd sandbox runs skill scripts with (python3 is
#    3.12 on Ubuntu 24.04, which the avito vendored parser targets).
#  - ffmpeg (with ffprobe): the transcribe connector measures a long recording (or a
#    video), finds its silences and cuts it into audio chunks the dictation endpoint
#    won't truncate; the `video` / `video-link` skills use it too.
#  - Chrome's shared libraries: the browser connector downloads Chrome-for-Testing on
#    its first fetch, but that binary links against the system NSS/NSPR, ATK, AT-SPI,
#    CUPS and X libraries, which a slim base doesn't have. Without them Chrome dies at
#    load ("libnspr4.so: cannot open shared object file") and every browser.fetch fails
#    — silently, since the connector doesn't log the launch error. The last line is
#    what Chrome dlopen()s at runtime (GBM, ALSA, xkbcommon — ldd doesn't list those)
#    plus its text rendering stack and a base font.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 libcurl4 \
        curl wget python3 ffmpeg \
        libnss3 libnspr4 libatk1.0-0t64 libatk-bridge2.0-0t64 libatspi2.0-0t64 \
        libcups2t64 libxcomposite1 libxdamage1 libxfixes3 libxrandr2 libdrm2 \
        libgbm1 libasound2t64 libxkbcommon0 libpango-1.0-0 libcairo2 fonts-liberation \
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
USER albert
ENTRYPOINT ["albert-entrypoint"]

# Development mounts a binary built on the host. Both stages share system libraries.
FROM runtime-base AS development
COPY albert.dev.toml /usr/share/albert/defaults/albert.toml

FROM runtime-base AS runtime
COPY albert.toml /usr/share/albert/defaults/albert.toml
COPY --from=build /albert /usr/local/bin/albert
