#!/usr/bin/env bash
set -euo pipefail

# manim-live installer script
# Usage: curl -sSL https://raw.githubusercontent.com/.../install.sh | bash

REPO="manim-live/manim-live"
BINARY="manim-live"

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

echo -e "${BLUE}${BOLD}==>${NC} Installing ${BOLD}${BINARY}${NC}..."

# Determine installation directory
INSTALL_DIR="${HOME}/.local/bin"
if [ "$(id -u)" -eq 0 ]; then
    INSTALL_DIR="/usr/local/bin"
fi
mkdir -p "${INSTALL_DIR}"

# Detect OS and architecture
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "${ARCH}" in
    x86_64|amd64)
        TARGET_ARCH="x86_64"
        ;;
    aarch64|arm64)
        TARGET_ARCH="aarch64"
        ;;
    *)
        echo -e "${RED}Error: Unsupported architecture: ${ARCH}${NC}" >&2
        exit 1
        ;;
esac

case "${OS}" in
    linux)
        TARGET_OS="unknown-linux-gnu"
        ;;
    darwin)
        TARGET_OS="apple-darwin"
        ;;
    *)
        echo -e "${RED}Error: Unsupported operating system: ${OS}${NC}" >&2
        exit 1
        ;;
esac

TARGET="${TARGET_ARCH}-${TARGET_OS}"

# Check if prebuilt release can be downloaded, or build from source
SUCCESS=false

if command -v curl >/dev/null 2>&1; then
    LATEST_TAG=$(curl -sSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || true)
    if [ -n "${LATEST_TAG}" ]; then
        DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/${BINARY}-${TARGET}.tar.gz"
        echo -e "${BLUE}-->${NC} Attempting to download release ${LATEST_TAG} for ${TARGET}..."
        TMP_DIR=$(mktemp -d)
        if curl -sSL -f "${DOWNLOAD_URL}" -o "${TMP_DIR}/archive.tar.gz" 2>/dev/null; then
            tar -xzf "${TMP_DIR}/archive.tar.gz" -C "${TMP_DIR}"
            mv "${TMP_DIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
            chmod +x "${INSTALL_DIR}/${BINARY}"
            rm -rf "${TMP_DIR}"
            SUCCESS=true
        fi
    fi
fi

if [ "${SUCCESS}" = false ]; then
    if command -v cargo >/dev/null 2>&1; then
        echo -e "${YELLOW}-->${NC} Prebuilt release not found or network restricted. Building from source via Cargo..."
        if [ -f "Cargo.toml" ]; then
            cargo build --release
            cp target/release/${BINARY} "${INSTALL_DIR}/${BINARY}"
            chmod +x "${INSTALL_DIR}/${BINARY}"
            SUCCESS=true
        else
            echo -e "${BLUE}-->${NC} Installing via cargo install git..."
            cargo install --git "https://github.com/${REPO}" --locked
            SUCCESS=true
        fi
    else
        echo -e "${RED}Error: Prebuilt release could not be downloaded and 'cargo' is not installed.${NC}" >&2
        echo -e "Please install Rust and Cargo from https://rustup.rs/ and try again." >&2
        exit 1
    fi
fi

echo -e "${GREEN}${BOLD}✔${NC} Successfully installed ${BOLD}${BINARY}${NC} to ${INSTALL_DIR}/${BINARY}!"

# Verify PATH
if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
    echo -e "${YELLOW}${BOLD}Note:${NC} ${INSTALL_DIR} is not in your PATH."
    echo -e "Add it to your shell profile by running:"
    echo -e "  export PATH=\"\$PATH:${INSTALL_DIR}\""
fi

echo -e "\n${BOLD}Quick Start:${NC}"
echo -e "  ${BINARY} scene.py"
