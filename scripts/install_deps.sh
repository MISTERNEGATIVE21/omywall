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

# Identify actual non-root user if invoked with sudo
REAL_USER="${SUDO_USER:-$USER}"

# Request Sudo Permissions if not root
if [ "$EUID" -ne 0 ]; then
    echo -e "${YELLOW}[!] Requesting sudo permissions to install required system packages...${NC}"
    sudo -v || { echo -e "${RED}[!] Sudo authentication failed. Exiting.${NC}"; exit 1; }
fi

run_aur_helper() {
    local helper="$1"
    shift
    if [ "$EUID" -eq 0 ] && [ -n "$SUDO_USER" ] && [ "$SUDO_USER" != "root" ]; then
        sudo -u "$SUDO_USER" "$helper" "$@"
    else
        "$helper" "$@"
    fi
}

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
        echo -e "${CYAN}[+] Installing packages via yay (Arch Linux / Hyprland)...${NC}"
        run_aur_helper yay -S --needed --noconfirm mpvpaper mpv ffmpeg electron jq libnotify hyprland hyprlock || {
            echo -e "${YELLOW}[!] yay installation encountered warnings. Trying pacman for core packages...${NC}"
            sudo pacman -S --needed --noconfirm mpv ffmpeg jq libnotify hyprland hyprlock electron || true
            run_aur_helper yay -S --needed --noconfirm mpvpaper || true
        }
        ;;
    paru)
        echo -e "${CYAN}[+] Installing packages via paru (Arch Linux / Hyprland)...${NC}"
        run_aur_helper paru -S --needed --noconfirm mpvpaper mpv ffmpeg electron jq libnotify hyprland hyprlock || {
            echo -e "${YELLOW}[!] paru installation encountered warnings. Trying pacman for core packages...${NC}"
            sudo pacman -S --needed --noconfirm mpv ffmpeg jq libnotify hyprland hyprlock electron || true
            run_aur_helper paru -S --needed --noconfirm mpvpaper || true
        }
        ;;
    pacman)
        echo -e "${CYAN}[+] Installing core packages via pacman...${NC}"
        sudo pacman -S --needed --noconfirm mpv ffmpeg jq libnotify hyprland hyprlock electron || true
        if ! command -v mpvpaper &> /dev/null; then
            echo -e "${YELLOW}[!] Building mpvpaper from source (AUR helper not found)...${NC}"
            sudo pacman -S --needed --noconfirm meson ninja wlroots wayland-protocols pkgconf gcc || true
            rm -rf /tmp/mpvpaper-build
            git clone https://github.com/GhostKey/mpvpaper.git /tmp/mpvpaper-build
            cd /tmp/mpvpaper-build
            meson setup build
            ninja -C build
            sudo ninja -C build install
        fi
        ;;
    apt)
        echo -e "${CYAN}[+] Installing via apt (Debian / Ubuntu / Pop!_OS)...${NC}"
        sudo apt-get update
        sudo apt-get install -y mpv ffmpeg electron jq libnotify-bin hyprlock build-essential meson ninja-build cmake pkg-config libwlroots-dev libwayland-dev || true
        if ! command -v mpvpaper &> /dev/null; then
            echo -e "${CYAN}[+] Building mpvpaper for Wayland...${NC}"
            rm -rf /tmp/mpvpaper-build
            git clone https://github.com/GhostKey/mpvpaper.git /tmp/mpvpaper-build
            cd /tmp/mpvpaper-build
            meson setup build
            ninja -C build
            sudo ninja -C build install
        fi
        ;;
    dnf)
        echo -e "${CYAN}[+] Installing via dnf (Fedora / Nobara / RHEL)...${NC}"
        sudo dnf install -y mpv ffmpeg jq libnotify hyprlock meson ninja-build wayland-protocols-devel wlroots-devel || true
        if ! command -v mpvpaper &> /dev/null; then
            echo -e "${CYAN}[+] Building mpvpaper...${NC}"
            rm -rf /tmp/mpvpaper-build
            git clone https://github.com/GhostKey/mpvpaper.git /tmp/mpvpaper-build
            cd /tmp/mpvpaper-build
            meson setup build
            ninja -C build
            sudo ninja -C build install
        fi
        ;;
    zypper)
        echo -e "${CYAN}[+] Installing via zypper (openSUSE)...${NC}"
        sudo zypper install -y mpv ffmpeg jq libnotify-tools || true
        ;;
    nix)
        echo -e "${CYAN}[+] Installing via Nix...${NC}"
        nix-env -iA nixos.mpvpaper nixos.mpv nixos.ffmpeg nixos.electron nixos.jq || true
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
echo -e "${GREEN}  All dependencies checked & processed successfully! ${NC}"
echo -e "${GREEN}=====================================================${NC}"
