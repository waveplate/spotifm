# Static Musl Build with GStreamer

This document details how **spotifm** achieves a 100% standalone, statically-linked binary with **Musl libc** and **GStreamer**, the underlying technical challenges, and how to trigger the build with a single command.

---

## 🚀 Quick Install & Distribution
 
### 1. One-Line Installer
 
Install the static Musl binary and player assets directly with:
 
```sh
curl -fsSL https://raw.githubusercontent.com/waveplate/spotifm/v3.1.0-alpha/install.sh | bash
```
 
This automatically places `spotifm` into `/usr/local/bin` and copies the web player into `/usr/local/share/spotifm/player` (or user paths if rootless).
 
### 2. One-Command Build
 
To build the static Musl binary and extract it to `./target/x86_64-unknown-linux-musl/release/spotifm`:
 
```sh
./scripts/build-static-musl.sh
```
 
### 3. Generate Release Archive (.tar.gz)
 
To package the compiled static binary, web player directory, default configuration, and SHA256 checksums into `./dist/`:
 
```sh
./scripts/package-release.sh
```
 
Or build and package in a single step:
 
```sh
./scripts/package-release.sh --build
```
 
---

## 🔍 Technical Architecture & Pitfalls Solved

Building GStreamer statically with Musl libc is notoriously tricky due to four fundamental conflicts between GStreamer's default dynamic loading architecture and Musl's static linking behavior:

### 1. The GNU `libintl` / `gettext` Weak-Symbol SIGSEGV Trap
* **Root Cause:** Standard Alpine static GLib (`glib-static`) packages link against GNU `gettext` (`libintl.a`). GNU `libintl` uses weak function pointers (`#pragma weak pthread_rwlock_unlock`, `pthread_cond_broadcast`, etc.) for thread synchronization. When statically linked with Musl, weak references resolve to `NULL` (`0x0000000000000000`). At runtime during `gst_init()`, `g_dgettext()` calls `libintl_dcigettext()`, which attempts to jump to the `NULL` function pointer and causes an immediate segmentation fault.
* **Resolution:** Compile GLib and GStreamer from source with **`-Dnls=disabled`**. This completely eliminates the GNU `libintl` dependency, allowing GLib to bypass weak lock function pointers and rely cleanly on Musl.

### 2. Static Plugin Dead-Stripping
* **Root Cause:** GStreamer usually relies on `dlopen()` to discover and load shared library plugins (`.so`) at runtime. When compiling static archives (`.a`), the linker discards unreferenced plugin symbols because nothing directly calls their internal registration functions prior to `gst_init()`.
* **Resolution:**
  1. Generate static plugin archives and declare them in [`src/gst_plugins.c`](../src/gst_plugins.c) with `GST_PLUGIN_STATIC_DECLARE` and `GST_PLUGIN_STATIC_REGISTER`.
  2. Compile and link this plugin registration shim via `build.rs`.
  3. Pass `-Wl,--whole-archive` specifically for the static plugin archives (`libgstcoreelements.a`, `libgstapp.a`, `libgstaudioconvert.a`, `libgstaudioresample.a`, `libgstplayback.a`, `libgsttypefindfunctions.a`, `libgstogg.a`, `libgstopus.a`, `libgstvorbis.a`, `libgstaudioparsers.a`, `libgstautodetect.a`, `libgstlame.a`) so the linker retains all symbols.

### 3. Disabling Dynamic Plugin Loading & Registry Scanning
* In a purely static binary, dynamic plugin scanning (`dlopen`) is bypassed because all required audio elements (`appsrc`, `audioconvert`, `audioresample`, `lamemp3enc`, `opusenc`, `vorbisenc`, `oggmux`, `appsink`) are baked directly into the binary.
* At runtime, `GST_REGISTRY_DISABLE=yes` is set to bypass external file-system plugin scanning.

### 4. Linker Circular Dependencies & Topological Ordering
* GStreamer, GLib, and audio codec dependencies (`libmp3lame`, `libogg`, `libvorbis`, `libvorbisenc`, `libopus`, `libpcre2-8`, `libffi`, `libz`) have circular cross-references.
* When compiling with `+crt-static`, all native libraries must be enclosed in `-Wl,-Bstatic -Wl,--start-group ... -Wl,--end-group`.

---

## 📦 Required Static Dependencies

| Library | Version | Purpose |
|---|---|---|
| **Musl libc** | 1.2.x | Lightweight, statically linkable C runtime |
| **GLib** | 2.82+ | Core types & event loop (built with `-Dnls=disabled`) |
| **GStreamer Core** | 1.24+ | Multimedia framework (`gstreamer-full` static) |
| **gst-plugins-base** | 1.24+ | `app`, `audioconvert`, `audioresample`, `ogg`, `vorbis`, `opus`, `playback`, `typefind` |
| **gst-plugins-good** | 1.24+ | `lame`, `audioparsers`, `autodetect` |
| **libopus** | 1.5.x | Opus audio codec (`libopus.a`) |
| **libmp3lame** | 3.100+ | MP3 audio encoder (`libmp3lame.a`) |
| **libogg / libvorbis** | 1.3.x | Ogg container and Vorbis audio encoder |
| **pcre2 / libffi / zlib** | Latest | GLib dependencies |

---

## 🛠️ Verification

You can verify that the generated binary has zero shared library dependencies:

```sh
$ file target/x86_64-unknown-linux-musl/release/spotifm
target/x86_64-unknown-linux-musl/release/spotifm: ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), static-pie linked, stripped

$ ldd target/x86_64-unknown-linux-musl/release/spotifm
statically linked
```
