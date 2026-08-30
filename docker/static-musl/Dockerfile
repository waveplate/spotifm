# Multi-stage Dockerfile for building Spotifm with fully static GStreamer on Musl (Alpine)

# ==============================================================================
# Stage 1: Build static dependencies (libopus, GLib without NLS, GStreamer static)
# ==============================================================================
FROM alpine:3.21 AS gst-builder

RUN apk add --no-cache \
    build-base \
    git \
    meson \
    samurai \
    pkgconf \
    bison \
    flex \
    pcre2-dev \
    libffi-dev \
    zlib-dev \
    zlib-static \
    libogg-dev \
    libogg-static \
    libvorbis-dev \
    libvorbis-static \
    lame-dev \
    alsa-lib-dev \
    curl

WORKDIR /build

# 1. Build libopus static archive
RUN curl -sSL https://downloads.xiph.org/releases/opus/opus-1.5.2.tar.gz | tar -xz && \
    cd opus-1.5.2 && \
    CFLAGS="-O3 -fomit-frame-pointer" ./configure --enable-static --disable-shared --disable-extra-programs --prefix=/opt/local-static && \
    make -j$(nproc) install

# 2. Build GLib static archive with NLS disabled (prevents GNU gettext weak symbol crash on Musl)
RUN git clone --depth 1 --branch 2.82.5 https://gitlab.gnome.org/GNOME/glib.git glib-src && \
    cd glib-src && \
    meson setup build-static \
      --buildtype=release \
      -Doptimization=3 \
      -Db_ndebug=true \
      --default-library=static \
      -Dintrospection=disabled \
      -Dnls=disabled \
      -Dtests=false \
      -Dlibmount=disabled \
      -Dselinux=disabled \
      --prefix=/opt/glib-static && \
    ninja -C build-static install

# 3. Build GStreamer monorepo static libraries and plugins
ENV PKG_CONFIG_PATH="/opt/glib-static/lib/pkgconfig:/opt/local-static/lib/pkgconfig"

RUN git clone --depth 1 --branch 1.24.12 https://gitlab.freedesktop.org/gstreamer/gstreamer.git gst-src && \
    cd gst-src && \
    meson setup build-static \
      --buildtype=release \
      -Doptimization=3 \
      -Db_ndebug=true \
      --default-library=static \
      -Dgst-full=enabled \
      -Dgst-full-target-type=static_library \
      -Dnls=disabled \
      -Dintrospection=disabled \
      -Ddoc=disabled \
      -Dexamples=disabled \
      -Dtests=disabled \
      -Dtools=disabled \
      -Dgstreamer:tools=disabled \
      -Dgstreamer:benchmarks=disabled \
      -Dauto_features=disabled \
      -Dbase=enabled \
      -Dgood=enabled \
      -Dbad=disabled \
      -Dugly=disabled \
      -Dlibav=disabled \
      -Ddevtools=disabled \
      -Dges=disabled \
      -Drtsp_server=disabled \
      -Dvaapi=disabled \
      -Dsharp=disabled \
      -Drs=disabled \
      -Dpython=disabled \
      -Dgst-plugins-base:app=enabled \
      -Dgst-plugins-base:audioconvert=enabled \
      -Dgst-plugins-base:audioresample=enabled \
      -Dgst-plugins-base:playback=enabled \
      -Dgst-plugins-base:typefind=enabled \
      -Dgst-plugins-base:ogg=enabled \
      -Dgst-plugins-base:vorbis=enabled \
      -Dgst-plugins-base:opus=enabled \
      -Dgst-plugins-good:lame=enabled \
      -Dgst-plugins-good:audioparsers=enabled \
      -Dgst-plugins-good:autodetect=enabled \
      -Dgst-full-libraries=app,audio \
      --prefix=/opt/gstreamer-static && \
    ninja -C build-static install

# ==============================================================================
# Stage 2: Build static Rust application
# ==============================================================================
FROM rust:alpine AS rust-builder

RUN apk add --no-cache \
    gcc \
    musl-dev \
    pkgconf \
    file \
    pcre2-dev \
    pcre2-static \
    zlib-dev \
    zlib-static \
    libffi-dev \
    libogg-dev \
    libogg-static \
    libvorbis-dev \
    libvorbis-static \
    lame-dev \
    alsa-lib-dev

COPY --from=gst-builder /opt/local-static /opt/local-static
COPY --from=gst-builder /opt/glib-static /opt/glib-static
COPY --from=gst-builder /opt/gstreamer-static /opt/gstreamer-static

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY build.rs ./build.rs
COPY src ./src

ENV PKG_CONFIG_PATH="/opt/glib-static/lib/pkgconfig:/opt/local-static/lib/pkgconfig:/opt/gstreamer-static/lib/pkgconfig:/opt/gstreamer-static/lib/gstreamer-1.0/pkgconfig"
ENV PKG_CONFIG_ALL_STATIC=1

ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="\
-C target-cpu=x86-64-v2 \
-C target-feature=+crt-static \
-L /opt/glib-static/lib \
-L /opt/gstreamer-static/lib \
-L /opt/gstreamer-static/lib/gstreamer-1.0 \
-L /opt/local-static/lib \
-C link-arg=-Wl,-Bstatic \
-C link-arg=-Wl,--start-group \
-C link-arg=-Wl,--whole-archive \
-C link-arg=-lgst_plugins_init \
-C link-arg=-lgstcoreelements \
-C link-arg=-lgstapp \
-C link-arg=-lgstaudioconvert \
-C link-arg=-lgstaudioresample \
-C link-arg=-lgstplayback \
-C link-arg=-lgsttypefindfunctions \
-C link-arg=-lgstogg \
-C link-arg=-lgstopus \
-C link-arg=-lgstvorbis \
-C link-arg=-lgstaudioparsers \
-C link-arg=-lgstautodetect \
-C link-arg=-lgstlame \
-C link-arg=-Wl,--no-whole-archive \
-C link-arg=-lgstapp-1.0 \
-C link-arg=-lgstaudio-1.0 \
-C link-arg=-lgstpbutils-1.0 \
-C link-arg=-lgsttag-1.0 \
-C link-arg=-lgstriff-1.0 \
-C link-arg=-lgstvideo-1.0 \
-C link-arg=-lgstbase-1.0 \
-C link-arg=-lgstreamer-1.0 \
-C link-arg=-lgio-2.0 \
-C link-arg=-lgobject-2.0 \
-C link-arg=-lgmodule-2.0 \
-C link-arg=-lglib-2.0 \
-C link-arg=-lmp3lame \
-C link-arg=-logg \
-C link-arg=-lvorbis \
-C link-arg=-lvorbisenc \
-C link-arg=-lopus \
-C link-arg=-lpcre2-8 \
-C link-arg=-lffi \
-C link-arg=-lz \
-C link-arg=-latomic \
-C link-arg=-lm \
-C link-arg=-ldl \
-C link-arg=-lc \
-C link-arg=-Wl,--end-group"

RUN cargo build --target x86_64-unknown-linux-musl --release

# ==============================================================================
# Stage 3: Minimal runtime container
# ==============================================================================
FROM alpine:3.21

RUN apk add --no-cache ca-certificates

WORKDIR /app

COPY --from=rust-builder /app/target/x86_64-unknown-linux-musl/release/spotifm /usr/local/bin/spotifm
COPY config.toml /config/config.toml
COPY player /app/player

EXPOSE 3333
EXPOSE 3443
EXPOSE 8898

# Disable GStreamer dynamic plugin scanning at runtime
ENV GST_REGISTRY_DISABLE=yes

ENTRYPOINT ["/usr/local/bin/spotifm"]
CMD ["--conf", "/config/config.toml", "--no-browser"]
