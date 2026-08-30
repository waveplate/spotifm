FROM rust:1.96.0-alpine AS builder

RUN apk add --no-cache \
      build-base \
      ca-certificates \
      cmake \
      gstreamer-dev \
      gst-plugins-base-dev \
      perl \
      pkgconf

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

# GStreamer is provided by Alpine as shared libraries, so the Rust binary must
# use the dynamic musl runtime instead of the target's default static CRT.
ENV RUSTFLAGS="-C target-feature=-crt-static"

RUN cargo build --release --locked

FROM alpine:3.23

RUN apk add --no-cache \
      ca-certificates \
      gstreamer \
      gst-plugins-base \
      gst-plugins-good \
      libgcc \
      openssl

WORKDIR /app

COPY --from=builder /app/target/release/spotifm /usr/local/bin/spotifm
COPY config.toml /config/config.toml
COPY player /app/player

EXPOSE 3333
EXPOSE 3443
EXPOSE 8898

ENTRYPOINT ["/usr/local/bin/spotifm"]
CMD ["--conf", "/config/config.toml", "--no-browser"]
