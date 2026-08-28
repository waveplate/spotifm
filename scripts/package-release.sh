#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# package-release.sh - Package static Musl spotifm binary and player directory
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Default values
DEFAULT_OUTPUT_DIR="${ROOT_DIR}/dist"
OUTPUT_DIR="${DEFAULT_OUTPUT_DIR}"
TARGET="x86_64-unknown-linux-musl"
CUSTOM_VERSION=""
CUSTOM_BINARY=""
RUN_BUILD=false
RUN_BUILD_PLAYER=false

# Helper functions
log_info() {
    echo -e "\033[1;34m==>\033[0m \033[1m$1\033[0m"
}

log_success() {
    echo -e "\033[1;32m==>\033[0m \033[1;32m$1\033[0m"
}

log_warn() {
    echo -e "\033[1;33m[Warning]\033[0m $1" >&2
}

log_error() {
    echo -e "\033[1;31m[Error]\033[0m $1" >&2
}

show_help() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Packages the static Musl spotifm binary, web player directory, and release
assets into a distributable .tar.gz archive with SHA256 checksums.

Options:
  -v, --version <VERSION>   Specify the release version (e.g. 3.1.0-alpha).
                            Default: extracted from Cargo.toml.
  -o, --output-dir <DIR>    Directory where the release .tar.gz will be saved.
                            Default: ./dist
  -b, --binary <PATH>       Path to static spotifm binary.
                            Default: ./target/x86_64-unknown-linux-musl/release/spotifm
  -t, --target <TRIPLE>     Target architecture/platform triple.
                            Default: x86_64-unknown-linux-musl
  --build                   Run ./scripts/build-static-musl.sh before packaging.
  --build-player            Run ./player/build.sh before packaging.
  -h, --help                Show this help message.

Examples:
  $(basename "$0")
  $(basename "$0") --build
  $(basename "$0") --version 3.1.0-alpha --output-dir ./dist
EOF
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        -v|--version)
            CUSTOM_VERSION="$2"
            shift 2
            ;;
        -o|--output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        -b|--binary)
            CUSTOM_BINARY="$2"
            shift 2
            ;;
        -t|--target)
            TARGET="$2"
            shift 2
            ;;
        --build)
            RUN_BUILD=true
            shift
            ;;
        --build-player)
            RUN_BUILD_PLAYER=true
            shift
            ;;
        -h|--help)
            show_help
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            show_help
            exit 1
            ;;
    esac
done

# Resolve version
if [[ -n "${CUSTOM_VERSION}" ]]; then
    VERSION="${CUSTOM_VERSION#v}"
else
    if [[ -f "${ROOT_DIR}/Cargo.toml" ]]; then
        VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "${ROOT_DIR}/Cargo.toml" | head -n 1)"
    else
        log_error "Could not locate Cargo.toml to extract version. Specify with --version."
        exit 1
    fi
fi

if [[ -z "${VERSION}" ]]; then
    log_error "Failed to resolve version."
    exit 1
fi

# Resolve binary path
if [[ -n "${CUSTOM_BINARY}" ]]; then
    BINARY_PATH="${CUSTOM_BINARY}"
else
    BINARY_PATH="${ROOT_DIR}/target/${TARGET}/release/spotifm"
fi

# Optional build steps
if [[ "${RUN_BUILD_PLAYER}" == true ]]; then
    log_info "Building web player assets..."
    if [[ -x "${ROOT_DIR}/player/build.sh" ]]; then
        "${ROOT_DIR}/player/build.sh"
    elif [[ -f "${ROOT_DIR}/player/build.sh" ]]; then
        bash "${ROOT_DIR}/player/build.sh"
    else
        log_warn "player/build.sh not found, skipping player build."
    fi
fi

if [[ "${RUN_BUILD}" == true ]]; then
    log_info "Building static Musl binary..."
    "${ROOT_DIR}/scripts/build-static-musl.sh"
fi

# Verify static binary exists
if [[ ! -f "${BINARY_PATH}" ]]; then
    log_error "Static binary not found at: ${BINARY_PATH}"
    log_error "To compile the static Musl binary, run:"
    echo "  ./scripts/build-static-musl.sh" >&2
    echo "  or run this script with '--build': $(basename "$0") --build" >&2
    exit 1
fi

if [[ ! -x "${BINARY_PATH}" ]]; then
    log_info "Setting executable permission on ${BINARY_PATH}..."
    chmod +x "${BINARY_PATH}"
fi

# Verify player directory exists
PLAYER_DIR="${ROOT_DIR}/player"
if [[ ! -d "${PLAYER_DIR}" ]]; then
    log_error "Player directory not found at: ${PLAYER_DIR}"
    exit 1
fi

if [[ ! -f "${PLAYER_DIR}/index.html" ]]; then
    log_warn "player/index.html not found inside ${PLAYER_DIR}. The web player might not serve properly."
fi

# Prepare staging directory
TMP_STAGE="$(mktemp -d -t spotifm-pkg-XXXXXX)"
cleanup() {
    rm -rf "${TMP_STAGE}"
}
trap cleanup EXIT

log_info "Packaging Spotifm v${VERSION} (${TARGET})..."

# Copy artifacts into staging area
mkdir -p "${TMP_STAGE}"
cp "${BINARY_PATH}" "${TMP_STAGE}/spotifm"
chmod 755 "${TMP_STAGE}/spotifm"

# Copy player directory
cp -r "${PLAYER_DIR}" "${TMP_STAGE}/player"
chmod -R u+rwX,go+rX "${TMP_STAGE}/player"

# Copy optional metadata files if present
if [[ -f "${ROOT_DIR}/config.toml" ]]; then
    cp "${ROOT_DIR}/config.toml" "${TMP_STAGE}/config.toml"
fi

if [[ -f "${ROOT_DIR}/README.md" ]]; then
    cp "${ROOT_DIR}/README.md" "${TMP_STAGE}/README.md"
fi

if [[ -f "${ROOT_DIR}/LICENSE" ]]; then
    cp "${ROOT_DIR}/LICENSE" "${TMP_STAGE}/LICENSE"
fi

# Create output directory
mkdir -p "${OUTPUT_DIR}"

PACKAGE_BASE="spotifm-v${VERSION}-${TARGET}"
TARBALL_NAME="${PACKAGE_BASE}.tar.gz"
TARBALL_PATH="${OUTPUT_DIR}/${TARBALL_NAME}"

# Create tarball archive (contents at archive root for clean extraction)
log_info "Creating archive: ${TARBALL_NAME}..."
tar -czf "${TARBALL_PATH}" -C "${TMP_STAGE}" \
    spotifm \
    player \
    $(test -f "${TMP_STAGE}/config.toml" && echo "config.toml") \
    $(test -f "${TMP_STAGE}/README.md" && echo "README.md") \
    $(test -f "${TMP_STAGE}/LICENSE" && echo "LICENSE")

# Create a generic alias archive (e.g. spotifm-linux-x86_64-musl.tar.gz)
GENERIC_TARBALL_NAME="spotifm-linux-x86_64-musl.tar.gz"
GENERIC_TARBALL_PATH="${OUTPUT_DIR}/${GENERIC_TARBALL_NAME}"
cp -f "${TARBALL_PATH}" "${GENERIC_TARBALL_PATH}"

# Generate SHA256 checksums
log_info "Generating SHA256 checksums..."
(
    cd "${OUTPUT_DIR}"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "${TARBALL_NAME}" > "${TARBALL_NAME}.sha256"
        sha256sum "${GENERIC_TARBALL_NAME}" > "${GENERIC_TARBALL_NAME}.sha256"
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "${TARBALL_NAME}" > "${TARBALL_NAME}.sha256"
        shasum -a 256 "${GENERIC_TARBALL_NAME}" > "${GENERIC_TARBALL_NAME}.sha256"
    else
        log_warn "Neither sha256sum nor shasum is available. Checksum file not created."
    fi
)

# Archive details
ARCHIVE_SIZE="$(du -h "${TARBALL_PATH}" | cut -f1)"
CHECKSUM=""
if [[ -f "${TARBALL_PATH}.sha256" ]]; then
    CHECKSUM="$(cut -d' ' -f1 < "${TARBALL_PATH}.sha256")"
fi

echo ""
log_success "Release package created successfully!"
echo "------------------------------------------------------------"
echo "  Archive:   ${TARBALL_PATH}"
echo "  Size:      ${ARCHIVE_SIZE}"
if [[ -n "${CHECKSUM}" ]]; then
    echo "  SHA256:    ${CHECKSUM}"
fi
echo "  Generic:   ${GENERIC_TARBALL_PATH}"
echo "------------------------------------------------------------"
echo "Archive Contents:"
tar -ztvf "${TARBALL_PATH}"
echo "------------------------------------------------------------"
