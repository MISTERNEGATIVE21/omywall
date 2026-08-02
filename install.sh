#!/usr/bin/env bash
set -e

echo "🌌 Rebuilding & Installing OMYWALL Wallpaper Engine in the system..."

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_PNG_DIR="$HOME/.local/share/icons/hicolor/512x512/apps"
ICON_SVG_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"
ASSETS_DEST="$HOME/.local/share/omywall/assets"

echo "🧹 Cleaning up old binaries, legacy files, and obsolete asset caches..."
rm -f "$BIN_DIR/omywall" "$BIN_DIR/omywall-picker" "$BIN_DIR/omarchy-wall"
rm -f "$DESKTOP_DIR/omywall.desktop" "$DESKTOP_DIR/omarchy-wall.desktop"
rm -f "$ICON_PNG_DIR/omywall.png" "$ICON_SVG_DIR/omywall.svg" "$ICON_PNG_DIR/omarchy-wall.png" "$ICON_SVG_DIR/omarchy-wall.svg"
rm -rf "$ASSETS_DEST" /tmp/omywall_thumbs /tmp/omywall_web_layer.py /tmp/omywall_web_app.js

mkdir -p "$BIN_DIR" "$DESKTOP_DIR" "$ICON_PNG_DIR" "$ICON_SVG_DIR" "$ASSETS_DEST"

echo "Building release binary..."
cargo build --release

echo "Installing clean binary to $BIN_DIR/omywall..."
install -m 755 "target/release/omywall" "$BIN_DIR/omywall"

echo "Installing desktop application entry..."
if [ -f "omywall.desktop" ]; then
    cp "omywall.desktop" "$DESKTOP_DIR/omywall.desktop"
fi

echo "Installing application icons..."
if [ -f "assets/omywall.png" ]; then
    cp "assets/omywall.png" "$ICON_PNG_DIR/omywall.png"
fi

if [ -f "assets/omywall.svg" ]; then
    cp "assets/omywall.svg" "$ICON_SVG_DIR/omywall.svg"
fi

echo "Installing fresh application web assets and 3D presets..."
if [ -d "assets" ]; then
    cp -r assets/* "$ASSETS_DEST/" 2>/dev/null || true
fi

echo "Installing quick launcher script to $BIN_DIR/omywall-picker..."
if [ -f "scripts/omywall-picker" ]; then
    install -m 755 "scripts/omywall-picker" "$BIN_DIR/omywall-picker"
fi

HYPR_CONF="$HOME/.config/hypr/hyprland.conf"
if [ -f "$HYPR_CONF" ]; then
    if ! grep -q "omywall-picker" "$HYPR_CONF"; then
        echo "" >> "$HYPR_CONF"
        echo "# OMYWALL Wallpaper Engine Quick Launcher (Super+Alt+Space)" >> "$HYPR_CONF"
        echo "bind = SUPER ALT, SPACE, exec, omywall-picker" >> "$HYPR_CONF"
        echo "Registered Super+Alt+Space keybinding in $HYPR_CONF"
    fi
fi

if command -v update-desktop-database &> /dev/null; then
    update-desktop-database "$DESKTOP_DIR" &> /dev/null || true
fi

if command -v gtk-update-icon-cache &> /dev/null; then
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" &> /dev/null || true
fi

echo ""
echo "=========================================================="
echo "✅ OMYWALL Wallpaper Engine successfully installed!"
echo "   Binary:  $BIN_DIR/omywall"
echo "   Desktop: $DESKTOP_DIR/omywall.desktop"
echo "   Icon:    $ICON_SVG_DIR/omywall.svg"
echo "=========================================================="
