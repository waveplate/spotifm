#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# build-static-musl.sh - One-command static Musl + GStreamer build for spotifm
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
OUTPUT_DIR="${ROOT_DIR}/target/x86_64-unknown-linux-musl/release"
OUTPUT_BIN="${OUTPUT_DIR}/spotifm"
IMAGE_TAG="spotifm-static-musl-toolchain:latest"
CACHE_PREFIX="${SPOTIFM_MUSL_CACHE_PREFIX:-spotifm-static-musl}"
CONTAINER_ID=""

cleanup() {
    if [[ -n "${CONTAINER_ID}" ]]; then
        docker rm -f "${CONTAINER_ID}" >/dev/null || true
    fi
}
trap cleanup EXIT

echo "==> Building Spotifm static Musl binary with GStreamer..."

if ! command -v docker >/dev/null 2>&1; then
    echo "Error: docker is required to perform the static Musl build." >&2
    exit 1
fi

mkdir -p "${OUTPUT_DIR}"

# Cache the compiler and native libraries independently of application changes.
docker build \
    -f "${ROOT_DIR}/docker/static-musl/Dockerfile" \
    --target rust-toolchain \
    -t "${IMAGE_TAG}" \
    "${ROOT_DIR}"

# Keep Cargo downloads and compiled dependencies across container runs. Mount
# only build inputs, so other repository files (including local Cargo config)
# cannot affect the container build. Cargo handles locking the shared caches.
echo "==> Building with persistent Cargo caches (${CACHE_PREFIX})..."
CONTAINER_ID="$(docker create \
    --mount "type=bind,src=${ROOT_DIR}/Cargo.toml,dst=/app/Cargo.toml,readonly" \
    --mount "type=bind,src=${ROOT_DIR}/Cargo.lock,dst=/app/Cargo.lock,readonly" \
    --mount "type=bind,src=${ROOT_DIR}/build.rs,dst=/app/build.rs,readonly" \
    --mount "type=bind,src=${ROOT_DIR}/src,dst=/app/src,readonly" \
    --mount "type=volume,src=${CACHE_PREFIX}-target,dst=/app/target" \
    --mount "type=volume,src=${CACHE_PREFIX}-registry,dst=/usr/local/cargo/registry" \
    --mount "type=volume,src=${CACHE_PREFIX}-git,dst=/usr/local/cargo/git" \
    "${IMAGE_TAG}" \
    cargo build --target x86_64-unknown-linux-musl --release --locked)"
docker start --attach "${CONTAINER_ID}"
BUILD_STATUS="$(docker wait "${CONTAINER_ID}")"
if [[ "${BUILD_STATUS}" != "0" ]]; then
    echo "Error: static Musl build failed (exit ${BUILD_STATUS})." >&2
    exit "${BUILD_STATUS}"
fi

# Extract the compiled static binary
echo "==> Extracting static binary to ${OUTPUT_BIN}..."
docker cp "${CONTAINER_ID}:/app/target/x86_64-unknown-linux-musl/release/spotifm" "${OUTPUT_BIN}"
docker rm -f "${CONTAINER_ID}" >/dev/null
CONTAINER_ID=""

chmod +x "${OUTPUT_BIN}"

echo "==> Verifying binary..."
if command -v file >/dev/null 2>&1; then
    file "${OUTPUT_BIN}"
fi

if command -v ldd >/dev/null 2>&1; then
    ldd "${OUTPUT_BIN}" || true
fi

echo "==> Testing binary execution (--help):"
"${OUTPUT_BIN}" --help | head -n 12

echo ""
echo "✅ Build completed successfully!"
echo "   Static binary available at: ${OUTPUT_BIN}"
