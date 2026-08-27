#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# build-static-musl.sh - One-command static Musl + GStreamer build for spotifm
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
OUTPUT_DIR="${ROOT_DIR}/target/x86_64-unknown-linux-musl/release"
OUTPUT_BIN="${OUTPUT_DIR}/spotifm"
IMAGE_TAG="spotifm-static-musl-builder:latest"

echo "==> Building Spotifm static Musl binary with GStreamer..."

if ! command -v docker >/dev/null 2>&1; then
    echo "Error: docker is required to perform the static Musl build." >&2
    exit 1
fi

mkdir -p "${OUTPUT_DIR}"

# Build the static builder image
docker build \
    -f "${ROOT_DIR}/docker/static-musl/Dockerfile" \
    --target rust-builder \
    -t "${IMAGE_TAG}" \
    "${ROOT_DIR}"

# Extract the compiled static binary
echo "==> Extracting static binary to ${OUTPUT_BIN}..."
CONTAINER_ID="$(docker create "${IMAGE_TAG}")"
docker cp "${CONTAINER_ID}:/app/target/x86_64-unknown-linux-musl/release/spotifm" "${OUTPUT_BIN}"
docker rm -f "${CONTAINER_ID}" >/dev/null

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
