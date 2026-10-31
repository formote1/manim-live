#!/usr/bin/env bash
# ==============================================================================
#  manim-live — Installer & Dependency Setup Script
#  https://github.com/manim-live/manim-live
# ==============================================================================
set -euo pipefail

# ANSI Color Palette
CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
PURPLE='\033[0;35m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m' # No Color

# Determine installation directory
INSTALL_DIR="${HOME}/.local/bin"
if [ "$(id -u)" -eq 0 ]; then
    INSTALL_DIR="/usr/local/bin"
fi
mkdir -p "${INSTALL_DIR}"

log_info() {
    echo -e "${CYAN}${BOLD}==>${NC} $1"
}

log_success() {
    echo -e "${GREEN}${BOLD}✔${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}${BOLD}⚠${NC} $1"
}

log_error() {
    echo -e "${RED}${BOLD}✖${NC} $1" >&2
}

echo -e "${PURPLE}${BOLD}"
cat << "EOF"
    __  ___            _             __    _           
   /  |/  /___ _____  (_)___ ___    / /   (_)   _____  
  / /|_/ / __ `/ __ \/ / __ `__ \  / /   / / | / / _ \ 
 / /  / / /_/ / / / / / / / / / / / /___/ /| |/ /  __/ 
/_/  /_/\__,_/_/ /_/_/_/ /_/ /_/ /_____/_/ |___/\___/  
EOF
echo -e "${NC}${CYAN}${BOLD}Interactive TUI Studio for Python's Manim Engine${NC}"
echo -e "${DIM}====================================================${NC}\n"

# 1. Detect Operating System and Package Manager
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
PKG_MANAGER=""

if [ "${OS}" = "linux" ]; then
    if command -v pacman >/dev/null 2>&1; then
        PKG_MANAGER="pacman"
    elif command -v apt-get >/dev/null 2>&1; then
        PKG_MANAGER="apt"
    elif command -v dnf >/dev/null 2>&1; then
        PKG_MANAGER="dnf"
    elif command -v zypper >/dev/null 2>&1; then
        PKG_MANAGER="zypper"
    elif command -v apk >/dev/null 2>&1; then
        PKG_MANAGER="apk"
    fi
elif [ "${OS}" = "darwin" ]; then
    if command -v brew >/dev/null 2>&1; then
        PKG_MANAGER="brew"
    fi
fi

# Helper for installing packages with sudo if needed
run_pkg_install() {
    local pkgs=("$@")
    if [ ${#pkgs[@]} -eq 0 ]; then return 0; fi

    local SUDO=""
    if [ "$(id -u)" -ne 0 ] && command -v sudo >/dev/null 2>&1; then
        SUDO="sudo"
    fi

    case "${PKG_MANAGER}" in
        apt)
            ${SUDO} apt-get update -qq
            ${SUDO} apt-get install -y "${pkgs[@]}"
            ;;
        pacman)
            ${SUDO} pacman -S --noconfirm --needed "${pkgs[@]}"
            ;;
        dnf)
            ${SUDO} dnf install -y "${pkgs[@]}"
            ;;
        zypper)
            ${SUDO} zypper install -y "${pkgs[@]}"
            ;;
        apk)
            ${SUDO} apk add "${pkgs[@]}"
            ;;
        brew)
            brew install "${pkgs[@]}"
            ;;
        *)
            log_warn "Unknown package manager. Please manually install: ${pkgs[*]}"
            return 1
            ;;
    esac
}

# 2. Check and Install FFmpeg (required for video playback & frame extraction)
log_info "Checking dependencies..."

if command -v ffmpeg >/dev/null 2>&1; then
    log_success "FFmpeg found: $(ffmpeg -version | head -n 1 | awk '{print $1, $2, $3}')"
else
    log_warn "FFmpeg not found. Attempting automatic installation..."
    case "${PKG_MANAGER}" in
        apt|pacman|dnf|zypper|apk|brew)
            run_pkg_install "ffmpeg" || true
            ;;
    esac
    if command -v ffmpeg >/dev/null 2>&1; then
        log_success "FFmpeg successfully installed!"
    else
        log_error "FFmpeg is required. Please install it via your system package manager."
    fi
fi

# 3. Check and Install micro (recommended terminal editor)
if command -v micro >/dev/null 2>&1; then
    log_success "Micro editor found: $(micro -version 2>/dev/null | head -n 1 || echo 'micro')"
else
    log_info "Installing recommended terminal editor 'micro'..."
    MICRO_INSTALLED=false
    if [ -n "${PKG_MANAGER}" ]; then
        run_pkg_install "micro" >/dev/null 2>&1 && MICRO_INSTALLED=true || true
    fi
    if [ "${MICRO_INSTALLED}" = false ]; then
        # Fallback to official micro standalone binary installer
        if curl -sSL https://getmic.ro | bash >/dev/null 2>&1; then
            if [ -f "./micro" ]; then
                mv ./micro "${INSTALL_DIR}/micro"
                chmod +x "${INSTALL_DIR}/micro"
                MICRO_INSTALLED=true
            fi
        fi
    fi

    if command -v micro >/dev/null 2>&1 || [ -f "${INSTALL_DIR}/micro" ]; then
        log_success "Micro editor installed successfully to ${INSTALL_DIR}/micro"
    else
        log_warn "'micro' could not be auto-installed. Existing editors (vim/nvim/nano) can be configured in Settings."
    fi
fi

# 4. Check Python 3 and Manim Community Engine
if command -v manim >/dev/null 2>&1; then
    log_success "Manim found: $(manim --version 2>/dev/null || echo 'installed')"
else
    log_warn "Manim executable not found in PATH."
    if command -v pipx >/dev/null 2>&1; then
        log_info "Installing Manim Community via pipx..."
        pipx install manim >/dev/null 2>&1 && log_success "Manim installed via pipx!" || true
    elif command -v uv >/dev/null 2>&1; then
        log_info "Installing Manim Community via uv..."
        uv tool install manim >/dev/null 2>&1 && log_success "Manim installed via uv!" || true
    elif [ "${PKG_MANAGER}" = "pacman" ]; then
        log_info "Installing python-manim via pacman..."
        run_pkg_install "python-manim" >/dev/null 2>&1 && log_success "Manim installed via pacman!" || true
    fi

    if ! command -v manim >/dev/null 2>&1; then
        echo -e "   ${DIM}Tip: You can install Manim in your preferred Python environment:${NC}"
        echo -e "     pipx install manim  ${DIM}(recommended)${NC}"
        echo -e "     pip install manim   ${DIM}(inside a virtualenv)${NC}"
    fi
fi

# 5. Ensure Rust / Cargo is installed
if command -v cargo >/dev/null 2>&1; then
    log_success "Rust / Cargo found: $(cargo --version | head -n 1)"
else
    log_info "Rust toolchain not found. Installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    if [ -f "${HOME}/.cargo/env" ]; then
        # shellcheck disable=SC1091
        source "${HOME}/.cargo/env"
    fi
fi

# 6. Build and Install manim-live
log_info "Building and installing ${BOLD}manim-live${NC}..."

if [ -f "Cargo.toml" ] && [ -d "src" ]; then
    # Local build inside repository
    cargo build --release
    install -m 755 target/release/manim-live "${INSTALL_DIR}/manim-live"
else
    # Standalone installation via Git
    TMP_DIR="$(mktemp -d)"
    log_info "Cloning repository to temporary directory..."
    git clone --depth 1 https://github.com/manim-live/manim-live.git "${TMP_DIR}"
    (
        cd "${TMP_DIR}"
        cargo build --release
        install -m 755 target/release/manim-live "${INSTALL_DIR}/manim-live"
    )
    rm -rf "${TMP_DIR}"
fi

# 7. Verification & PATH Check
if [ -x "${INSTALL_DIR}/manim-live" ]; then
    log_success "manim-live successfully installed at: ${BOLD}${INSTALL_DIR}/manim-live${NC}"
else
    log_error "Installation failed. Check compiler output above."
    exit 1
fi

# Check PATH
PATH_OK=false
IFS=':' read -ra ADDR <<< "$PATH"
for p in "${ADDR[@]}"; do
    if [ "$p" = "${INSTALL_DIR}" ]; then
        PATH_OK=true
        break
    fi
done

if [ "${PATH_OK}" = false ]; then
    echo ""
    log_warn "${INSTALL_DIR} is not currently in your PATH environment variable."
    echo -e "   Add it to your shell configuration file (e.g. ~/.bashrc, ~/.zshrc):"
    echo -e "     ${BOLD}export PATH=\"\$HOME/.local/bin:\$PATH\"${NC}"
    echo -e "   Then run: ${BOLD}source ~/.bashrc${NC} (or restart your terminal)\n"
fi

echo -e "\n${GREEN}${BOLD}Setup complete! Enjoy live animation prototyping with manim-live!${NC}"
echo -e "${BOLD}Get started immediately:${NC}"
echo -e "  ${CYAN}manim-live scene.py${NC}              # Open studio on scene.py"
echo -e "  ${CYAN}manim-live scene.py MyScene${NC}      # Open studio focused on a specific Scene\n"
