#!/usr/bin/env bash
set -e

echo "🌌 Rebuilding & Installing OMYWALL Wallpaper Engine in the system..."

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_PNG_DIR="$HOME/.local/share/icons/hicolor/512x512/apps"
ICON_SVG_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"

mkdir -p "$BIN_DIR" "$DESKTOP_DIR" "$ICON_PNG_DIR" "$ICON_SVG_DIR"

echo "Building release binary..."
cargo build --release

echo "Installing binary to $BIN_DIR/omywall..."
install -m 755 "target/release/omywall" "$BIN_DIR/omywall"

echo "Removing legacy omarchy-wall binary aliases and desktop residue..."
rm -f "$BIN_DIR/omarchy-wall" "$DESKTOP_DIR/omarchy-wall.desktop" "$ICON_PNG_DIR/omarchy-wall.png" "$ICON_SVG_DIR/omarchy-wall.svg"

echo "Installing desktop application entry..."
if [ -f "omywall.desktop" ]; then
    cp "omywall.desktop" "$DESKTOP_DIR/omywall.desktop"
fi

echo "Installing application icons..."
if [ -f "assets/omarchy-wall.png" ]; then
    cp "assets/omarchy-wall.png" "$ICON_PNG_DIR/omywall.png"
fi

if [ -f "assets/omarchy-wall.svg" ]; then
    cp "assets/omarchy-wall.svg" "$ICON_SVG_DIR/omywall.svg"
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
