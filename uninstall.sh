#!/usr/bin/env bash
# ==============================================================================
#  manim-live — Application Uninstaller
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

ASSUME_YES=false
PURGE_ALL=false

# Parse command line flags
for arg in "$@"; do
    case "${arg}" in
        -y|--yes|--force)
            ASSUME_YES=true
            ;;
        --purge)
            PURGE_ALL=true
            ;;
        -h|--help)
            echo "manim-live uninstaller"
            echo "Usage: ./uninstall.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  -y, --yes, --force    Confirm uninstallation without prompting"
            echo "  --purge               Purge everything including build artifacts in current repo"
            echo "  -h, --help            Show this help message"
            exit 0
            ;;
    esac
done

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
echo -e "${NC}${RED}${BOLD}Application Uninstaller & Purge Script${NC}"
echo -e "${DIM}====================================================${NC}\n"

# 1. Discover target paths
TARGETS=()

add_target_if_exists() {
    local target="$1"
    if [ -e "${target}" ]; then
        TARGETS+=("${target}")
    fi
}

# Standard binary locations
add_target_if_exists "${HOME}/.local/bin/manim-live"
add_target_if_exists "${HOME}/.cargo/bin/manim-live"
add_target_if_exists "/usr/local/bin/manim-live"

# Configuration directory
if [ -n "${XDG_CONFIG_HOME:-}" ] && [ -d "${XDG_CONFIG_HOME}/manim-live" ]; then
    add_target_if_exists "${XDG_CONFIG_HOME}/manim-live"
else
    add_target_if_exists "${HOME}/.config/manim-live"
fi

# Cache directory
if [ -n "${XDG_CACHE_HOME:-}" ] && [ -d "${XDG_CACHE_HOME}/manim-live" ]; then
    add_target_if_exists "${XDG_CACHE_HOME}/manim-live"
else
    add_target_if_exists "${HOME}/.cache/manim-live"
fi

# Optional local repository build directory if purging
if [ "${PURGE_ALL}" = true ] && [ -d "target" ]; then
    add_target_if_exists "$(pwd)/target"
fi

if [ ${#TARGETS[@]} -eq 0 ]; then
    log_warn "No installed manim-live binaries or configuration files were found."
    echo "  Checked locations:"
    echo "    • ~/.local/bin/manim-live"
    echo "    • ~/.cargo/bin/manim-live"
    echo "    • /usr/local/bin/manim-live"
    echo "    • ~/.config/manim-live"
    echo "    • ~/.cache/manim-live"
    echo -e "\n${GREEN}${BOLD}✔${NC} System is already clean."
    exit 0
fi

echo -e "${BOLD}The following installed items were detected and will be deleted:${NC}"
for item in "${TARGETS[@]}"; do
    if [ -d "${item}" ]; then
        echo -e "  ${YELLOW}[Directory]${NC} ${item}"
    else
        echo -e "  ${CYAN}[Binary]${NC}    ${item}"
    fi
done
echo ""

# 2. Confirmation prompt
if [ "${ASSUME_YES}" = false ]; then
    read -r -p "$(echo -e "${BOLD}Are you sure you want to completely uninstall manim-live? [y/N]: ${NC}")" response
    case "${response}" in
        [yY][eE][sS]|[yY])
            echo ""
            ;;
        *)
            echo -e "\n${YELLOW}Uninstallation cancelled.${NC} No files were deleted."
            exit 0
            ;;
    esac
fi

# 3. Deletion execution
REMOVED=0
FAILED=0

for item in "${TARGETS[@]}"; do
    if [ -w "${item}" ] || [ -w "$(dirname "${item}")" ]; then
        rm -rf "${item}"
        log_success "Deleted: ${item}"
        ((REMOVED+=1))
    else
        # Try with sudo if not writable
        if command -v sudo >/dev/null 2>&1; then
            log_warn "Elevated permissions required to delete ${item}..."
            if sudo rm -rf "${item}"; then
                log_success "Deleted: ${item} (via sudo)"
                ((REMOVED+=1))
            else
                log_error "Failed to delete: ${item}"
                ((FAILED+=1))
            fi
        else
            log_error "Permission denied: ${item} (run with root privileges to delete)"
            ((FAILED+=1))
        fi
    fi
done

echo ""
if [ "${FAILED}" -eq 0 ]; then
    echo -e "${GREEN}${BOLD}✔ Complete uninstallation finished successfully!${NC}"
    echo -e "  All manim-live binaries, configurations, and cache files have been removed.\n"
else
    echo -e "${YELLOW}${BOLD}⚠ Uninstallation finished with warnings (${REMOVED} removed, ${FAILED} failed).${NC}\n"
fi
