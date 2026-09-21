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
FROM ubuntu:24.04 AS build

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
    cargo build --release --locked \
    && cp target/release/albert /albert

# ---- runtime ----
FROM ubuntu:24.04 AS runtime

ENV DEBIAN_FRONTEND=noninteractive
# Runtime libraries + tools:
#  - ca-certificates, libssl3, libcurl4: TLS + the libcurl the binary links (and the
#    search backend uses).
#  - curl, wget, python3: what the forkd sandbox runs skill scripts with (python3 is
#    3.12 on Ubuntu 24.04, which the avito vendored parser targets).
#  - ffmpeg (with ffprobe): backs the `transcribe` skill — it measures a recording,
#    finds its silences and cuts it into chunks the dictation endpoint won't truncate.
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
    && groupadd --system albert \
    && useradd --system --no-create-home --gid albert --shell /usr/sbin/nologin albert \
    && useradd --system --no-create-home --gid albert --shell /usr/sbin/nologin albert-scripts \
    # Runtime state lives on the /data volume; a fresh named volume inherits this owner.
    && mkdir -p /data && chown albert:albert /data

# The Codex CLI, so skills can drive it directly on the same ChatGPT subscription
# Albert already authenticates with. One static musl binary (no node, no runtime deps)
# — pinned, because "latest" would make the image unreproducible.
ARG CODEX_VERSION=rust-v0.146.0
RUN curl -fsSL "https://github.com/openai/codex/releases/download/${CODEX_VERSION}/codex-x86_64-unknown-linux-musl.tar.gz" \
        | tar -xz -C /usr/local/bin \
    && mv /usr/local/bin/codex-x86_64-unknown-linux-musl /usr/local/bin/codex \
    && chmod 0755 /usr/local/bin/codex \
    && codex --version

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
COPY --from=build /albert /usr/local/bin/albert
# Persona + connector manifests + skills are baked in; mutable state lives on /data.
# albert.toml is deliberately NOT baked: the self-config skill edits it at runtime, and a
# baked copy lives in the image's ephemeral layer — writes would report success and then
# vanish on the next `up -d`. It is bind-mounted from an operator-owned file instead (see
# docker-compose.yml); the container will not start without that mount, which is the point.
COPY soul.md system.md ./
COPY config ./config
COPY skills ./skills
RUN chown -R albert:albert /opt/albert

ENV ALBERT_CONFIG=/opt/albert/albert.toml \
    KAERU_VAULT_PATH=/data/kaeru \
    OCTO_CODE_WORKSPACE=/data/workspace \
    # The browser connector's Chrome writes under $HOME (crashpad/NSS) and SIGTRAPs on a
    # missing one; point it at the writable volume.
    HOME=/data \
    OCTO_SKILLS_DIR=/opt/albert/skills \
    # CODEX_HOME on the volume so the CLI's own state survives rebuilds. Deliberately
    # NOT /data itself: codex would then share Albert's auth.json and rewrite it on
    # refresh. The `imagegen` skill drives codex on this subscription; forkd forwards
    # CODEX_HOME (config/connectors/forkd) and runs the script as `albert-scripts`, which
    # must read the token and write codex state here — so the linked auth.json is group-
    # readable and the dir group-writable. One-time link command: see docker-compose.yml.
    CODEX_HOME=/data/codex \
    RUST_LOG=albert=info,octo_rig=info,octo_connector_scheduler=info,octo_connector_telegram=info,octo_connector_caldav=info,octo_connector_storage=info,octo_connector_forkd=info,octo_connector_search=info,octo_connector_http=info,octo_connector_mail=info,octo_http_auth=info,octo_core=warn

# Run as the unprivileged service user. forkd still drops scripts to albert-scripts —
# from non-root that needs CAP_SETUID/CAP_SETGID, granted in docker-compose (cap_add).
# Running as `albert` (gid albert) means workspace files are group `albert` by default,
# so the dropped script user can read them without a setgid dance.
USER albert

ENTRYPOINT ["albert"]
