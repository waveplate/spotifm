#!/bin/bash

PATH=$PATH:/usr/local/cargo/bin
mkdir -p /tmp/spotify-fm/target
cargo build --manifest-path /tmp/spotify-fm/Cargo.toml --target-dir /tmp/spotify-fm/target --release
