#!/usr/bin/env bash
# ==============================================================================
# OMYWALL Wallpaper Engine - Multi-Distro Dependency Installer Script
# ==============================================================================

set -e

GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${CYAN}=====================================================${NC}"
echo -e "${CYAN}   OMYWALL Wallpaper Engine - Dependency Installer   ${NC}"
echo -e "${CYAN}=====================================================${NC}"

# Request Sudo Permissions if not root
if [ "$EUID" -ne 0 ]; then
    echo -e "${YELLOW}[!] Requesting sudo permissions to install required system packages...${NC}"
    sudo -v || { echo -e "${RED}[!] Sudo authentication failed. Exiting.${NC}"; exit 1; }
fi

detect_pkg_manager() {
    if command -v yay &> /dev/null; then
        echo "yay"
    elif command -v paru &> /dev/null; then
        echo "paru"
    elif command -v pacman &> /dev/null; then
        echo "pacman"
    elif command -v apt-get &> /dev/null; then
        echo "apt"
    elif command -v dnf &> /dev/null; then
        echo "dnf"
    elif command -v zypper &> /dev/null; then
        echo "zypper"
    elif command -v nix-env &> /dev/null; then
        echo "nix"
    else
        echo "unknown"
    fi
}

PKG_MGR=$(detect_pkg_manager)
echo -e "${GREEN}[+] Detected Package Manager:${NC} $PKG_MGR"

case "$PKG_MGR" in
    yay)
        echo -e "${CYAN}[+] Installing via yay (Arch Linux / Omarchy)...${NC}"
        yay -S --needed --noconfirm mpvpaper mpv ffmpeg electron jq libnotify hyprland
        ;;
    paru)
        echo -e "${CYAN}[+] Installing via paru (Arch Linux / Omarchy)...${NC}"
        paru -S --needed --noconfirm mpvpaper mpv ffmpeg electron jq libnotify hyprland
        ;;
    pacman)
        echo -e "${CYAN}[+] Installing via pacman (Arch Linux / Omarchy)...${NC}"
        sudo pacman -S --needed --noconfirm mpv ffmpeg jq libnotify hyprland
        echo -e "${YELLOW}[!] 'mpvpaper' is an AUR package. Installing mpvpaper from source...${NC}"
        if ! command -v mpvpaper &> /dev/null; then
            git clone https://github.com/GhostKey/mpvpaper.git /tmp/mpvpaper-build || true
            cd /tmp/mpvpaper-build
            meson setup build
            ninja -C build
            sudo ninja -C build install
        fi
        ;;
    apt)
        echo -e "${CYAN}[+] Installing via apt (Debian / Ubuntu / Pop!_OS)...${NC}"
        sudo apt-get update
        sudo apt-get install -y mpv ffmpeg electron jq libnotify-bin build-essential meson ninja-build cmake pkg-config libwlroots-dev libwayland-dev
        if ! command -v mpvpaper &> /dev/null; then
            echo -e "${CYAN}[+] Building mpvpaper for Wayland...${NC}"
            git clone https://github.com/GhostKey/mpvpaper.git /tmp/mpvpaper-build || true
            cd /tmp/mpvpaper-build
            meson setup build
            ninja -C build
            sudo ninja -C build install
        fi
        ;;
    dnf)
        echo -e "${CYAN}[+] Installing via dnf (Fedora / Nobara / RHEL)...${NC}"
        sudo dnf install -y mpv ffmpeg jq libnotify meson ninja-build wayland-protocols-devel wlroots-devel
        if ! command -v mpvpaper &> /dev/null; then
            echo -e "${CYAN}[+] Building mpvpaper...${NC}"
            git clone https://github.com/GhostKey/mpvpaper.git /tmp/mpvpaper-build || true
            cd /tmp/mpvpaper-build
            meson setup build
            ninja -C build
            sudo ninja -C build install
        fi
        ;;
    zypper)
        echo -e "${CYAN}[+] Installing via zypper (openSUSE)...${NC}"
        sudo zypper install -y mpv ffmpeg jq libnotify-tools
        ;;
    nix)
        echo -e "${CYAN}[+] Installing via Nix...${NC}"
        nix-env -iA nixos.mpvpaper nixos.mpv nixos.ffmpeg nixos.electron nixos.jq
        ;;
    *)
        echo -e "${RED}[!] Unsupported or unrecognized package manager.${NC}"
        echo -e "Please install the following packages manually:"
        echo -e "  - mpvpaper (or build from https://github.com/GhostKey/mpvpaper)"
        echo -e "  - mpv"
        echo -e "  - ffmpeg"
        echo -e "  - electron"
        echo -e "  - jq"
        echo -e "  - libnotify"
        exit 1
        ;;
esac

echo -e "${GREEN}=====================================================${NC}"
echo -e "${GREEN}  All dependencies installed successfully!          ${NC}"
echo -e "${GREEN}=====================================================${NC}"
