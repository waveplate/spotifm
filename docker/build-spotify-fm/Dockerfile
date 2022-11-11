FROM rust:1.62

RUN apt update && apt install -y libasound2-dev pkg-config libpulse-dev build-essential

COPY build.sh /tmp/build.sh

RUN mkdir -p /tmp/spotify-fm && \
    chmod +x /tmp/build.sh

ENTRYPOINT ["/tmp/build.sh"]
