#!/usr/bin/env bash
# ==============================================================================
# iSLET Installer
# Fast 1-command installer for iSLET (Wayland Dynamic Island / Media HUD)
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/3m44n/iSLET/main/install.sh | bash
#   or from local repo:
#   ./install.sh
# ==============================================================================

set -euo pipefail

REPO="3m44n/iSLET"
GITHUB_URL="https://github.com/${REPO}"

# Colors
if [ -t 1 ]; then
    BOLD="\033[1m"
    GREEN="\033[32m"
    BLUE="\033[34m"
    YELLOW="\033[33m"
    RED="\033[31m"
    RESET="\033[0m"
else
    BOLD=""
    GREEN=""
    BLUE=""
    YELLOW=""
    RED=""
    RESET=""
fi

info() {
    printf "${BLUE}${BOLD}[*]${RESET} %s\n" "$1"
}

success() {
    printf "${GREEN}${BOLD}[✓]${RESET} %s\n" "$1"
}

warn() {
    printf "${YELLOW}${BOLD}[!]${RESET} %s\n" "$1"
}

error() {
    printf "${RED}${BOLD}[✗]${RESET} %s\n" "$1" >&2
    exit 1
}

# 1. Detect environment
OS="$(uname -s)"
if [ "$OS" != "Linux" ]; then
    error "iSLET is built specifically for Linux Wayland compositors (detected: $OS)."
fi

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64)
        TARGET_ARCH="x86_64"
        ;;
    aarch64|arm64)
        TARGET_ARCH="aarch64"
        ;;
    *)
        error "Unsupported architecture: $ARCH. Supported: x86_64, aarch64."
        ;;
esac

# 2. Determine install prefix
if [ "$(id -u)" -eq 0 ]; then
    PREFIX="/usr/local"
    BIN_DIR="${PREFIX}/bin"
    DATA_DIR="${PREFIX}/share"
    SYSTEMD_DIR="${PREFIX}/lib/systemd/user"
else
    PREFIX="${HOME}/.local"
    BIN_DIR="${PREFIX}/bin"
    DATA_DIR="${XDG_DATA_HOME:-${HOME}/.local/share}"
    SYSTEMD_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user"
fi

mkdir -p "$BIN_DIR" "$DATA_DIR/applications" "$DATA_DIR/islet/themes" "$SYSTEMD_DIR"

info "Installing iSLET for ${BOLD}${TARGET_ARCH}-linux${RESET} into ${BOLD}${PREFIX}${RESET}..."

# 3. Check if running from local source or needs download
TMP_DIR="$(mktemp -d /tmp/islet-install-XXXXXX)"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

# Check if local files are present
if [ -f "./Cargo.toml" ] && [ -d "./crates/islet-ui" ]; then
    info "Detected local source directory. Checking for pre-built binaries..."
    if [ -f "target/release/islet" ] && [ -f "target/release/islet-ctl" ]; then
        info "Using existing release binaries in target/release/..."
        cp -f target/release/islet "$BIN_DIR/islet"
        cp -f target/release/islet-ctl "$BIN_DIR/islet-ctl"
    elif command -v cargo >/dev/null 2>&1; then
        info "Building from source with cargo (this may take a couple minutes)..."
        cargo build --release --workspace
        cp -f target/release/islet "$BIN_DIR/islet"
        cp -f target/release/islet-ctl "$BIN_DIR/islet-ctl"
    else
        error "No pre-built binary found and 'cargo' is not installed."
    fi

    # Install assets from local directory
    cp -f packaging/dev.islet.Overlay.desktop "$DATA_DIR/applications/"
    cp -f packaging/islet.service "$SYSTEMD_DIR/"
    cp -f assets/style.css "$DATA_DIR/islet/style.css"
    cp -f assets/theme-catppuccin-mocha.css "$DATA_DIR/islet/themes/"
    cp -f assets/theme-dark-glass.css "$DATA_DIR/islet/themes/"

else
    # Remote download from GitHub Releases
    info "Fetching latest release from GitHub (${REPO})..."
    
    LATEST_JSON=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null || echo "")
    
    DOWNLOAD_URL=""
    if [ -n "$LATEST_JSON" ]; then
        DOWNLOAD_URL=$(echo "$LATEST_JSON" | grep "browser_download_url" | grep "${TARGET_ARCH}-linux.tar.gz" | cut -d '"' -f 4 | head -n 1 || echo "")
    fi

    if [ -n "$DOWNLOAD_URL" ]; then
        info "Downloading pre-compiled release: $DOWNLOAD_URL"
        curl -fsSL "$DOWNLOAD_URL" -o "$TMP_DIR/islet.tar.gz"
        tar -xzf "$TMP_DIR/islet.tar.gz" -C "$TMP_DIR"

        cp -f "$TMP_DIR/islet" "$BIN_DIR/islet"
        cp -f "$TMP_DIR/islet-ctl" "$BIN_DIR/islet-ctl"
        
        [ -f "$TMP_DIR/dev.islet.Overlay.desktop" ] && cp -f "$TMP_DIR/dev.islet.Overlay.desktop" "$DATA_DIR/applications/"
        [ -f "$TMP_DIR/islet.service" ] && cp -f "$TMP_DIR/islet.service" "$SYSTEMD_DIR/"
        [ -f "$TMP_DIR/style.css" ] && cp -f "$TMP_DIR/style.css" "$DATA_DIR/islet/style.css"
        if [ -d "$TMP_DIR/themes" ]; then
            cp -rf "$TMP_DIR/themes/"* "$DATA_DIR/islet/themes/"
        fi
    else
        warn "No published GitHub release asset found for ${TARGET_ARCH} yet."
        if command -v cargo >/dev/null 2>&1; then
            info "Falling back to cloning and compiling from source with cargo..."
            git clone --depth 1 "$GITHUB_URL.git" "$TMP_DIR/source"
            cd "$TMP_DIR/source"
            cargo build --release --workspace
            cp -f target/release/islet "$BIN_DIR/islet"
            cp -f target/release/islet-ctl "$BIN_DIR/islet-ctl"
            cp -f packaging/dev.islet.Overlay.desktop "$DATA_DIR/applications/"
            cp -f packaging/islet.service "$SYSTEMD_DIR/"
            cp -f assets/style.css "$DATA_DIR/islet/style.css"
            cp -f assets/theme-catppuccin-mocha.css "$DATA_DIR/islet/themes/"
            cp -f assets/theme-dark-glass.css "$DATA_DIR/islet/themes/"
        else
            error "Could not find a pre-built binary and 'cargo' is not installed.\nPlease install Rust/Cargo (https://rustup.rs) or build manually."
        fi
    fi
fi

chmod +x "$BIN_DIR/islet" "$BIN_DIR/islet-ctl"

# 4. Success summary and tips
echo ""
success "iSLET installed successfully!"
echo ""
printf "  ${BOLD}Installed files:${RESET}\n"
printf "    • Binary:    %s/islet\n" "$BIN_DIR"
printf "    • CLI:       %s/islet-ctl\n" "$BIN_DIR"
printf "    • Desktop:   %s/dev.islet.Overlay.desktop\n" "$DATA_DIR/applications"
printf "    • Themes:    %s/islet/\n" "$DATA_DIR"
printf "    • Systemd:   %s/islet.service\n" "$SYSTEMD_DIR"
echo ""

# Check PATH
case ":$PATH:" in
    *:"$BIN_DIR":*) ;;
    *)
        warn "$BIN_DIR is not in your \$PATH."
        printf "   Add this to your ~/.bashrc, ~/.zshrc, or ~/.config/fish/config.fish:\n"
        printf "     export PATH=\"%s:\$PATH\"\n\n" "$BIN_DIR"
        ;;
esac

printf "${BOLD}Quick Start:${RESET}\n"
printf "  1. Test the daemon:           ${GREEN}islet &${RESET}\n"
printf "  2. Test CLI toggle:           ${GREEN}islet-ctl toggle${RESET}\n"
printf "  3. Start automatically:       ${GREEN}systemctl --user enable --now islet.service${RESET}\n"
printf "     or in Hyprland config:     ${GREEN}exec-once = islet${RESET}\n"
echo ""
