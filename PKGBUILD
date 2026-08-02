# Maintainer: MisterNegative <misternegative21@gmail.com>
pkgname=omywall
pkgver=0.1.0.r27.g2c5c9c9
pkgrel=1
pkgdesc="Ultra-Lightweight Hardware-Accelerated Video, Web 3D & Desktop Wallpaper Engine with NVIDIA/CUDA Acceleration & Hyprlock Screensaver Integration"
arch=('x86_64')
url="https://github.com/MISTERNEGATIVE21/omywall"
license=('MIT')
depends=('mpv' 'mpvpaper' 'ffmpeg' 'python' 'python-gobject' 'gtk3' 'gtk-layer-shell' 'webkit2gtk' 'libnotify' 'jq' 'hicolor-icon-theme')
makedepends=('cargo' 'git')
optdepends=(
    'hyprlock: Wayland screensaver lockscreen support'
    'electron: Fallback web wallpaper engine for desktop web streams'
    'hyprland: Wayland tiling compositor support'
    'sway: Sway/wlroots tiling compositor support'
)
source=("git+https://github.com/MISTERNEGATIVE21/omywall.git")
sha256sums=('SKIP')

pkgver() {
    cd "${srcdir}/${pkgname}" 2>/dev/null || cd "${pkgname}" 2>/dev/null || true
    local ver=$(git describe --long --tags 2>/dev/null | sed 's/^v//;s/\([^-]*-g\)/r\1/;s/-/./g')
    if [ -z "$ver" ]; then
        ver="0.1.0.r$(git rev-list --count HEAD 2>/dev/null || echo 1).g$(git rev-parse --short HEAD 2>/dev/null || echo dev)"
    fi
    echo "$ver"
}

build() {
    cd "${srcdir}/${pkgname}" 2>/dev/null || cd "${pkgname}" 2>/dev/null || true
    export CARGO_PROFILE_RELEASE_DEBUG=false
    export CARGO_BUILD_JOBS=$(nproc)
    cargo build --release
}

package() {
    cd "${srcdir}/${pkgname}" 2>/dev/null || cd "${pkgname}" 2>/dev/null || true

    install -Dm755 "target/release/${pkgname}" "${pkgdir}/usr/bin/${pkgname}"
    install -Dm644 "omywall.desktop" "${pkgdir}/usr/share/applications/${pkgname}.desktop"
    if [ -f "assets/omywall.svg" ]; then
        install -Dm644 "assets/omywall.svg" "${pkgdir}/usr/share/icons/hicolor/scalable/apps/${pkgname}.svg"
    fi
    install -Dm644 "README.md" "${pkgdir}/usr/share/doc/${pkgname}/README.md"

    # Install WebGL 3D Wallpapers & Assets
    mkdir -p "${pkgdir}/usr/share/omywall/assets"
    if [ -d "assets" ]; then
        cp -r assets/* "${pkgdir}/usr/share/omywall/assets/"
    fi

    # Install Quick Launcher Picker script
    if [ -f "scripts/omywall-picker" ]; then
        install -Dm755 "scripts/omywall-picker" "${pkgdir}/usr/bin/omywall-picker"
    fi
}
