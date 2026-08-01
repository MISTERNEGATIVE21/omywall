#!/usr/bin/env bash
# ==============================================================================
# OMYWALL Wallpaper Engine - Setup Doctor & Diagnostics Script
# ==============================================================================

GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${CYAN}=====================================================${NC}"
echo -e "${CYAN}   OMYWALL Wallpaper Engine - Diagnostic Doctor     ${NC}"
echo -e "${CYAN}=====================================================${NC}"
echo ""

check_tool() {
    local name="$1"
    local cmd="$2"
    local desc="$3"

    if command -v "$cmd" &> /dev/null; then
        local loc
        loc=$(command -v "$cmd")
        printf "  [${GREEN}OK${NC}]  ${CYAN}%-12s${NC} -> %s (${GREEN}%s${NC})\n         %s\n" "$name" "$loc" "INSTALLED" "$desc"
    else
        printf "  [${RED}FAIL${NC}] ${YELLOW}%-12s${NC} -> ${RED}NOT FOUND${NC}\n         %s\n" "$name" "$desc"
    fi
}

echo -e "${GREEN}[1] Checking Required & Optional Tool Dependencies:${NC}"
check_tool "mpvpaper" "mpvpaper" "Primary Wayland video wallpaper renderer (wlr-layer-shell)"
check_tool "mpv" "mpv" "Hardware-accelerated media player engine"
check_tool "ffmpeg" "ffmpeg" "Video thumbnail generator & media converter"
check_tool "electron" "electron" "Desktop web widgets & HTML5 wallpaper renderer"
check_tool "jq" "jq" "JSON processor for IPC communication"
check_tool "notify-send" "notify-send" "Desktop notification provider"
check_tool "hyprctl" "hyprctl" "Hyprland compositor controller"
echo ""

echo -e "${GREEN}[2] Checking Wayland Environment & Socket Paths:${NC}"
if [ -n "$WAYLAND_DISPLAY" ]; then
    echo -e "  [${GREEN}OK${NC}]  Wayland Session Active: $WAYLAND_DISPLAY"
else
    echo -e "  [${YELLOW}WARN${NC}] WAYLAND_DISPLAY variable is empty. Ensure you are running under Hyprland, Sway, or wlroots."
fi

if [ -n "$HYPRLAND_INSTANCE_SIGNATURE" ]; then
    echo -e "  [${GREEN}OK${NC}]  Hyprland Compositor Active (Signature: $HYPRLAND_INSTANCE_SIGNATURE)"
fi

RUN_SOCKET="${XDG_RUNTIME_DIR:-/tmp}/omywall.sock"
if [ -e "$RUN_SOCKET" ]; then
    echo -e "  [${GREEN}OK${NC}]  IPC Server Socket Found: $RUN_SOCKET"
else
    echo -e "  [${YELLOW}INFO${NC}] IPC Server Socket not active yet ($RUN_SOCKET)."
fi

echo ""
echo -e "${GREEN}[3] Checking Hardware Acceleration Drivers:${NC}"
if command -v vainfo &> /dev/null; then
    echo -e "  [${GREEN}OK${NC}]  VA-API Driver Available (Intel/AMD GPU)"
elif command -v nvidia-smi &> /dev/null; then
    echo -e "  [${GREEN}OK${NC}]  NVIDIA Driver Available (NVDEC GPU)"
else
    echo -e "  [${YELLOW}INFO${NC}] Generic GPU video acceleration active."
fi

echo ""
echo -e "${CYAN}=====================================================${NC}"
echo -e "${CYAN}  Diagnostic Check Completed.                       ${NC}"
echo -e "${CYAN}=====================================================${NC}"
