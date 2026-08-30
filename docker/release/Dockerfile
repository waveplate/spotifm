# Lightweight Spotifm runtime container using pre-built static Musl release
FROM alpine:3.21

ARG VERSION=v3.1.0
ARG REPO=waveplate/spotifm
ARG TARGET=x86_64-unknown-linux-musl

RUN apk add --no-cache ca-certificates curl tar && \
    mkdir -p /app /config /usr/local/share/spotifm /root/.local/share/spotifm /root/.cache/spotifm && \
    DOWNLOAD_URL="" && \
    for url in \
      "https://github.com/${REPO}/releases/download/${VERSION}/spotifm-${VERSION}-${TARGET}.tar.gz" \
      "https://github.com/${REPO}/releases/download/${VERSION}/spotifm-linux-x86_64-musl.tar.gz" \
      "https://github.com/${REPO}/releases/download/${VERSION}/spotifm-${VERSION#v}-${TARGET}.tar.gz" \
      "https://github.com/${REPO}/releases/latest/download/spotifm-linux-x86_64-musl.tar.gz"; \
    do \
      if curl -fsSL -I "$url" >/dev/null 2>&1; then \
        DOWNLOAD_URL="$url"; \
        break; \
      fi; \
    done && \
    if [ -z "$DOWNLOAD_URL" ]; then \
      DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/spotifm-${VERSION}-${TARGET}.tar.gz"; \
    fi && \
    echo "Downloading Spotifm release from ${DOWNLOAD_URL}..." && \
    curl -fsSL "${DOWNLOAD_URL}" -o /tmp/spotifm.tar.gz && \
    tar -xzf /tmp/spotifm.tar.gz -C /tmp/ && \
    mv /tmp/spotifm /usr/local/bin/spotifm && \
    chmod 755 /usr/local/bin/spotifm && \
    mv /tmp/player /app/player && \
    cp -r /app/player /usr/local/share/spotifm/player && \
    if [ -f /tmp/config.toml ]; then cp /tmp/config.toml /config/config.toml; fi && \
    rm -rf /tmp/* && \
    apk del curl tar

WORKDIR /app

EXPOSE 3333
EXPOSE 3443
EXPOSE 8898

# Disable GStreamer dynamic plugin scanning at runtime
ENV GST_REGISTRY_DISABLE=yes

ENTRYPOINT ["/usr/local/bin/spotifm"]
CMD ["--conf", "/config/config.toml", "--no-browser"]
