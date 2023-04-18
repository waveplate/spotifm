#!/bin/bash

PATH=$PATH:/usr/local/cargo/bin
mkdir -p /tmp/spotifm/target
cargo build --manifest-path /tmp/spotifm/Cargo.toml --target-dir /tmp/spotifm/target --release
