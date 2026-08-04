# Maintainer: MisterNegative <misternegative21@gmail.com>
pkgname=omywall
pkgver=5.0.0
pkgrel=1
pkgdesc="Ultra-Lightweight Hardware-Accelerated Video, Web 3D & Desktop Wallpaper Engine with NVIDIA/CUDA Acceleration & Hyprlock Screensaver Integration"
arch=('x86_64')
url="https://github.com/MISTERNEGATIVE21/omywall"
license=('MIT')
depends=('mpv' 'mpvpaper' 'ffmpeg' 'python' 'python-gobject' 'gtk3' 'gtk-layer-shell' 'webkit2gtk' 'libnotify' 'jq' 'hicolor-icon-theme')
makedepends=('cargo')
optdepends=(
    'hyprlock: Wayland screensaver lockscreen support'
    'hyprland: Wayland tiling compositor support'
    'sway: Sway/wlroots tiling compositor support'
)
source=("${pkgname}-${pkgver}.tar.gz::https://github.com/MISTERNEGATIVE21/omywall/archive/refs/tags/v${pkgver}.tar.gz")
sha256sums=('SKIP')

build() {
    cd "${srcdir}/${pkgname}-${pkgver}"
    export CARGO_PROFILE_RELEASE_DEBUG=false
    export CARGO_BUILD_JOBS=$(nproc)
    cargo build --release
}

package() {
    cd "${srcdir}/${pkgname}-${pkgver}"

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
