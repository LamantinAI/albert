FROM ubuntu:24.04 AS environment
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 libcurl4 python3 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system albert \
    && useradd --system --no-create-home --gid albert albert \
    && useradd --system --no-create-home --gid albert albert-scripts \
    && mkdir -p /data && chown albert:albert /data
FROM environment AS runtime
COPY albert /usr/local/bin/albert
COPY start.py /opt/fixture/start.py
USER albert
WORKDIR /data
ENV ALBERT_CONFIG=/data/albert.toml KAERU_VAULT_PATH=/data/kaeru
ENTRYPOINT ["python3", "/opt/fixture/start.py"]
