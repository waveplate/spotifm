# building spotifm

[documentation index](README.md) · [project readme](../README.md)

1. install rust with rustup, then load `cargo` into the current shell:

   ```sh
   curl https://sh.rustup.rs -sSf | sh
   source "$HOME/.cargo/env"
   ```

2. install openssl, pkg-config, and gstreamer development libraries:

   <details>
   <summary>package commands</summary>

   **debian / ubuntu / linux mint**

   > sudo apt install openssl pkg-config libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev

   **fedora**

   > sudo dnf install openssl pkgconf-pkg-config gstreamer1-devel gstreamer1-plugins-base-devel

   **rhel / centos stream / rocky linux / almalinux**

   > sudo dnf install openssl pkgconf-pkg-config gstreamer1-devel gstreamer1-plugins-base-devel

   **arch linux / manjaro / endeavouros**

   > sudo pacman -S openssl pkgconf gstreamer gst-plugins-base-libs

   **alpine linux**

   > sudo apk add openssl pkgconf gstreamer-dev gst-plugins-base-dev

   **opensuse**

   > sudo zypper install openssl pkg-config gstreamer-devel gstreamer-plugins-base-devel

   **gentoo**

   > sudo emerge --ask dev-libs/openssl virtual/pkgconfig media-libs/gstreamer media-libs/gst-plugins-base

   **void linux**

   > sudo xbps-install openssl pkg-config gstreamer1-devel gst-plugins-base1-devel

   **macos / homebrew**

   > brew install openssl pkg-config gstreamer

   **linuxbrew**

   > brew install openssl pkg-config gstreamer

   </details>

3. build spotifm:

   ```sh
   cargo build --release --locked
   ```

---

## static musl build

To build a 100% standalone, statically-linked binary without dynamic GStreamer runtime dependencies, see [static musl build guide](static-musl-build.md) or run:

```sh
./scripts/build-static-musl.sh
```

