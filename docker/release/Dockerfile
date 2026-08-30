# Lightweight Spotifm runtime container using the official install.sh script
FROM alpine:3.21

ARG VERSION=v3.1.0
ARG REPO=waveplate/spotifm

RUN apk add --no-cache bash ca-certificates curl tar && \
    mkdir -p /app /config /root/.local/share/spotifm /root/.cache/spotifm && \
    curl -fsSL "https://raw.githubusercontent.com/${REPO}/${VERSION}/install.sh" | bash -s -- --version "${VERSION}" --repo "${REPO}" && \
    ln -sf /usr/local/share/spotifm/player /app/player && \
    if [ -f /root/.config/spotifm/config.toml ]; then cp /root/.config/spotifm/config.toml /config/config.toml; fi && \
    apk del curl tar

WORKDIR /app

EXPOSE 3333
EXPOSE 3443
EXPOSE 8898

# Disable GStreamer dynamic plugin scanning at runtime
ENV GST_REGISTRY_DISABLE=yes

ENTRYPOINT ["/usr/local/bin/spotifm"]
CMD ["--conf", "/config/config.toml", "--no-browser"]
