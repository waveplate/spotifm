#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# install.sh - Official installer for Spotifm (Static Musl release)
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/waveplate/spotifm/main/install.sh | bash
#
# Environment variables / Options:
#   SPOTIFM_VERSION   Specific version to install (e.g. "3.1.0" or "latest")
#   SPOTIFM_REPO      GitHub repository (default: "waveplate/spotifm")
#   SPOTIFM_URL       Direct archive download URL (bypasses GitHub releases)
#   SPOTIFM_BIN_DIR   Destination for binary (default: /usr/local/bin or ~/.local/bin)
#   SPOTIFM_DATA_DIR  Destination for player assets (default: /usr/local/share or ~/.local/share)
#   SPOTIFM_NO_SUDO   Set to 1 to force installation to user directory without sudo
# ==============================================================================

# ANSI Color formatting
if [[ -t 1 ]]; then
    COLOR_RESET="\033[0m"
    COLOR_BOLD="\033[1m"
    COLOR_GREEN="\033[1;32m"
    COLOR_BLUE="\033[1;34m"
    COLOR_YELLOW="\033[1;33m"
    COLOR_RED="\033[1;31m"
    COLOR_CYAN="\033[1;36m"
else
    COLOR_RESET=""
    COLOR_BOLD=""
    COLOR_GREEN=""
    COLOR_BLUE=""
    COLOR_YELLOW=""
    COLOR_RED=""
    COLOR_CYAN=""
fi

log_info() {
    echo -e "${COLOR_BLUE}==>${COLOR_RESET} ${COLOR_BOLD}$1${COLOR_RESET}"
}

log_success() {
    echo -e "${COLOR_GREEN}==>${COLOR_RESET} ${COLOR_GREEN}$1${COLOR_RESET}"
}

log_warn() {
    echo -e "${COLOR_YELLOW}[Warning]${COLOR_RESET} $1" >&2
}

log_error() {
    echo -e "${COLOR_RED}[Error]${COLOR_RESET} $1" >&2
}

show_help() {
    cat <<EOF
Spotifm Installer

Installs the standalone static Musl spotifm binary and web player assets.

Usage:
  curl -fsSL https://raw.githubusercontent.com/waveplate/spotifm/main/install.sh | bash
  ./install.sh [OPTIONS]

Options:
  -v, --version <VER>      Version tag to install (default: latest).
  -b, --bin-dir <DIR>      Directory to install spotifm executable.
                           Default: /usr/local/bin (or ~/.local/bin if rootless)
  -d, --data-dir <DIR>     Directory to install spotifm/player assets.
                           Default: /usr/local/share (or ~/.local/share if rootless)
  --repo <OWNER/REPO>      GitHub repository (default: waveplate/spotifm)
  --url <URL>              Custom download URL for the .tar.gz package
  --no-sudo                Force rootless install to ~/.local/bin and ~/.local/share
  -h, --help               Show this help message

Environment Variables:
  SPOTIFM_VERSION, SPOTIFM_BIN_DIR, SPOTIFM_DATA_DIR, SPOTIFM_REPO, SPOTIFM_URL, SPOTIFM_NO_SUDO
EOF
}

# Configuration defaults
REPO="${SPOTIFM_REPO:-waveplate/spotifm}"
REQ_VERSION="${SPOTIFM_VERSION:-${VERSION:-latest}}"
CUSTOM_URL="${SPOTIFM_URL:-}"
REQ_BIN_DIR="${SPOTIFM_BIN_DIR:-${BIN_DIR:-}}"
REQ_DATA_DIR="${SPOTIFM_DATA_DIR:-${DATA_DIR:-}}"
NO_SUDO="${SPOTIFM_NO_SUDO:-${NO_SUDO:-0}}"

# Parse CLI arguments if passed (e.g. via `bash -s -- --option` or direct execution)
while [[ $# -gt 0 ]]; do
    case "$1" in
        -v|--version)
            REQ_VERSION="$2"
            shift 2
            ;;
        -b|--bin-dir)
            REQ_BIN_DIR="$2"
            shift 2
            ;;
        -d|--data-dir)
            REQ_DATA_DIR="$2"
            shift 2
            ;;
        --repo)
            REPO="$2"
            shift 2
            ;;
        --url)
            CUSTOM_URL="$2"
            shift 2
            ;;
        --no-sudo)
            NO_SUDO=1
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

# ------------------------------------------------------------------------------
# 1. Platform & Architecture Verification
# ------------------------------------------------------------------------------
OS="$(uname -s)"
case "${OS}" in
    Linux*)
        OS_TYPE="linux"
        ;;
    Darwin*)
        log_error "Pre-built static Musl releases are only available for Linux."
        echo "To build spotifm on macOS, clone the repository and run: cargo build --release" >&2
        exit 1
        ;;
    *)
        log_error "Unsupported operating system: ${OS}"
        echo "To build spotifm, clone the repository and run: cargo build --release" >&2
        exit 1
        ;;
esac

ARCH="$(uname -m)"
case "${ARCH}" in
    x86_64|amd64)
        TARGET_ARCH="x86_64"
        ;;
    *)
        log_error "Unsupported architecture: ${ARCH}"
        echo "Pre-built static Musl releases currently target x86_64 Linux." >&2
        echo "To compile for ${ARCH}, clone the repository and run: cargo build --release" >&2
        exit 1
        ;;
esac

# ------------------------------------------------------------------------------
# 2. HTTP Downloader Utility (curl or wget)
# ------------------------------------------------------------------------------
http_download() {
    local url="$1"
    local output="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fL --progress-bar "${url}" -o "${output}"
    elif command -v wget >/dev/null 2>&1; then
        wget -q --show-progress "${url}" -O "${output}"
    else
        log_error "Neither curl nor wget is installed. Please install one of them."
        exit 1
    fi
}

http_get_text() {
    local url="$1"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -H "Accept: application/vnd.github.v3+json" "${url}" 2>/dev/null || true
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- --header="Accept: application/vnd.github.v3+json" "${url}" 2>/dev/null || true
    fi
}

http_check_url() {
    local url="$1"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -I "${url}" >/dev/null 2>&1
    elif command -v wget >/dev/null 2>&1; then
        wget -q --spider "${url}" >/dev/null 2>&1
    else
        false
    fi
}

# ------------------------------------------------------------------------------
# 3. Resolve Download URL and Version
# ------------------------------------------------------------------------------
DOWNLOAD_URL=""
RESOLVED_VERSION=""

if [[ -n "${CUSTOM_URL}" ]]; then
    DOWNLOAD_URL="${CUSTOM_URL}"
    RESOLVED_VERSION="custom"
    log_info "Using custom download URL: ${DOWNLOAD_URL}"
else
    log_info "Resolving Spotifm release from ${REPO} (${REQ_VERSION})..."

    if [[ "${REQ_VERSION}" == "latest" ]]; then
        # Try fetching release information via GitHub API
        API_RESP="$(http_get_text "https://api.github.com/repos/${REPO}/releases/latest")"
        if [[ -n "${API_RESP}" ]]; then
            RESOLVED_VERSION="$(echo "${API_RESP}" | grep -o '"tag_name": *"[^"]*"' | head -n1 | cut -d'"' -f4 || true)"
            # Find candidate browser_download_url with musl or x86_64
            DOWNLOAD_URL="$(echo "${API_RESP}" | grep -o '"browser_download_url": *"[^"]*"' | cut -d'"' -f4 | grep -E 'x86_64.*musl.*\.tar\.gz|linux.*musl.*\.tar\.gz|musl.*\.tar\.gz|\.tar\.gz' | head -n 1 || true)"
        fi

        # Fallback if GitHub API was rate-limited or didn't return an asset url
        if [[ -z "${DOWNLOAD_URL}" ]]; then
            CANDIDATE_URLS=(
                "https://github.com/${REPO}/releases/latest/download/spotifm-linux-x86_64-musl.tar.gz"
                "https://github.com/${REPO}/releases/latest/download/spotifm-x86_64-unknown-linux-musl.tar.gz"
            )
            for candidate in "${CANDIDATE_URLS[@]}"; do
                if http_check_url "${candidate}"; then
                    DOWNLOAD_URL="${candidate}"
                    RESOLVED_VERSION="latest"
                    break
                fi
            done
        fi
    else
        TAG="${REQ_VERSION}"
        if [[ ! "${TAG}" =~ ^v ]]; then
            TAG="v${TAG}"
        fi
        VERSION_NO_V="${TAG#v}"
        RESOLVED_VERSION="${TAG}"

        # Check tagged release assets
        CANDIDATE_URLS=(
            "https://github.com/${REPO}/releases/download/${TAG}/spotifm-${TAG}-x86_64-unknown-linux-musl.tar.gz"
            "https://github.com/${REPO}/releases/download/${TAG}/spotifm-v${VERSION_NO_V}-x86_64-unknown-linux-musl.tar.gz"
            "https://github.com/${REPO}/releases/download/${TAG}/spotifm-${VERSION_NO_V}-x86_64-unknown-linux-musl.tar.gz"
            "https://github.com/${REPO}/releases/download/${TAG}/spotifm-linux-x86_64-musl.tar.gz"
            "https://github.com/${REPO}/releases/download/${TAG}/spotifm-x86_64-unknown-linux-musl.tar.gz"
            "https://github.com/${REPO}/releases/download/${TAG#v}/spotifm-${TAG#v}-x86_64-unknown-linux-musl.tar.gz"
        )

        for candidate in "${CANDIDATE_URLS[@]}"; do
            if http_check_url "${candidate}"; then
                DOWNLOAD_URL="${candidate}"
                break
            fi
        done
    fi
fi

if [[ -z "${DOWNLOAD_URL}" ]]; then
    log_error "Could not find a valid release package for ${REPO} (version: ${REQ_VERSION})."
    echo "Please check available releases at: https://github.com/${REPO}/releases" >&2
    exit 1
fi

log_info "Found package at: ${DOWNLOAD_URL}"

# ------------------------------------------------------------------------------
# 4. Resolve Installation Locations & Privileges
# ------------------------------------------------------------------------------
needs_root_privileges() {
    local target_dir="$1"
    if [[ -d "${target_dir}" ]]; then
        [[ ! -w "${target_dir}" ]]
    else
        local parent="${target_dir}"
        while [[ ! -d "${parent}" && "${parent}" != "/" ]]; do
            parent="$(dirname "${parent}")"
        done
        [[ ! -w "${parent}" ]]
    fi
}

SUDO_CMD=""
USER_INSTALL=false

if [[ "${NO_SUDO}" == "1" || "${NO_SUDO}" == "true" ]]; then
    USER_INSTALL=true
fi

# Determine default paths if not explicitly specified
if [[ "${USER_INSTALL}" == true ]]; then
    BIN_DIR="${REQ_BIN_DIR:-${HOME}/.local/bin}"
    DATA_DIR="${REQ_DATA_DIR:-${XDG_DATA_HOME:-${HOME}/.local/share}}"
else
    BIN_DIR="${REQ_BIN_DIR:-/usr/local/bin}"
    DATA_DIR="${REQ_DATA_DIR:-/usr/local/share}"
fi

# Check if target directories require sudo
if needs_root_privileges "${BIN_DIR}" || needs_root_privileges "${DATA_DIR}"; then
    if [[ "$(id -u)" -eq 0 ]]; then
        SUDO_CMD=""
    elif [[ "${USER_INSTALL}" == false ]] && command -v sudo >/dev/null 2>&1; then
        log_info "Administrator permissions (sudo) required to install to ${BIN_DIR} and ${DATA_DIR}"
        SUDO_CMD="sudo"
    else
        log_warn "Target directories require root access but sudo is unavailable or disabled."
        log_info "Switching to user-level installation (~/.local/bin and ~/.local/share)..."
        BIN_DIR="${HOME}/.local/bin"
        DATA_DIR="${XDG_DATA_HOME:-${HOME}/.local/share}"
        SUDO_CMD=""
    fi
fi

BIN_TARGET="${BIN_DIR}/spotifm"
PLAYER_TARGET="${DATA_DIR}/spotifm/player"

# ------------------------------------------------------------------------------
# 5. Download & Extract Archive
# ------------------------------------------------------------------------------
TMP_DIR="$(mktemp -d -t spotifm-install-XXXXXX)"
cleanup() {
    rm -rf "${TMP_DIR}"
}
trap cleanup EXIT

ARCHIVE_FILE="${TMP_DIR}/spotifm.tar.gz"
EXTRACT_DIR="${TMP_DIR}/extracted"
mkdir -p "${EXTRACT_DIR}"

log_info "Downloading Spotifm release package..."
http_download "${DOWNLOAD_URL}" "${ARCHIVE_FILE}"

log_info "Extracting archive..."
tar -xzf "${ARCHIVE_FILE}" -C "${EXTRACT_DIR}"

# Locate static binary
EXTRACTED_BIN="$(find "${EXTRACT_DIR}" -type f -name "spotifm" -not -path "*/player/*" | head -n 1 || true)"
if [[ -z "${EXTRACTED_BIN}" || ! -f "${EXTRACTED_BIN}" ]]; then
    log_error "Could not find 'spotifm' binary inside the downloaded archive."
    exit 1
fi

# Locate player directory
EXTRACTED_PLAYER="$(find "${EXTRACT_DIR}" -type d -name "player" | head -n 1 || true)"
if [[ -z "${EXTRACTED_PLAYER}" || ! -d "${EXTRACTED_PLAYER}" ]]; then
    log_error "Could not find 'player' directory inside the downloaded archive."
    exit 1
fi

if [[ ! -f "${EXTRACTED_PLAYER}/index.html" ]]; then
    log_warn "Extracted player directory does not contain index.html."
fi

# ------------------------------------------------------------------------------
# 6. Install Binary and Player Assets
# ------------------------------------------------------------------------------
log_info "Installing static binary to ${BIN_TARGET}..."
${SUDO_CMD} mkdir -p "${BIN_DIR}"
${SUDO_CMD} cp -f "${EXTRACTED_BIN}" "${BIN_TARGET}"
${SUDO_CMD} chmod 755 "${BIN_TARGET}"

log_info "Installing web player assets to ${PLAYER_TARGET}..."
${SUDO_CMD} mkdir -p "${DATA_DIR}/spotifm"
${SUDO_CMD} rm -rf "${PLAYER_TARGET}"
${SUDO_CMD} cp -r "${EXTRACTED_PLAYER}" "${PLAYER_TARGET}"
${SUDO_CMD} chmod -R u+rwX,go+rX "${PLAYER_TARGET}"

# Optional: Default user configuration hint
SAMPLE_CONFIG="${EXTRACT_DIR}/config.toml"
USER_CONFIG_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/spotifm"
USER_CONFIG_FILE="${USER_CONFIG_DIR}/config.toml"

if [[ -f "${SAMPLE_CONFIG}" && ! -f "${USER_CONFIG_FILE}" ]]; then
    mkdir -p "${USER_CONFIG_DIR}"
    cp -n "${SAMPLE_CONFIG}" "${USER_CONFIG_FILE}" 2>/dev/null || true
fi

# ------------------------------------------------------------------------------
# 7. Verification and Path Check
# ------------------------------------------------------------------------------
echo ""
log_success "Spotifm static Musl binary and player installed successfully!"
echo "------------------------------------------------------------"
echo "  Executable:  ${BIN_TARGET}"
echo "  Web Player:  ${PLAYER_TARGET}"
if [[ -f "${USER_CONFIG_FILE}" ]]; then
    echo "  Config:      ${USER_CONFIG_FILE}"
fi
echo "------------------------------------------------------------"

# Check if BIN_DIR is in PATH
if ! echo ":${PATH}:" | grep -q ":${BIN_DIR}:"; then
    log_warn "${BIN_DIR} is not in your current PATH."
    echo "To run 'spotifm' directly, add the directory to your PATH:"
    echo "  export PATH=\"${BIN_DIR}:\$PATH\""
    echo ""
fi

# Quick start tips
echo -e "${COLOR_CYAN}${COLOR_BOLD}Next Steps:${COLOR_RESET}"
echo "  1. Start Spotifm by running:"
echo "     ${COLOR_BOLD}spotifm${COLOR_RESET}"
echo ""
echo "  2. Connect your Spotify Premium account by visiting:"
echo "     ${COLOR_BOLD}http://127.0.0.1:3333/oauth${COLOR_RESET}"
echo ""
echo "  3. Open the web player:"
echo "     ${COLOR_BOLD}http://127.0.0.1:3333${COLOR_RESET}"
echo "------------------------------------------------------------"
