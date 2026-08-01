# Maintainer: MisterNegative <misternegative21@gmail.com>
pkgname=omywall
pkgver=0.1.0
pkgrel=1
pkgdesc="Universal Hardware-Accelerated Video, Stream & Workspace Wallpaper Engine for Wayland"
arch=('x86_64')
url="https://github.com/misternegative21/omywall"
license=('MIT')
depends=('mpv' 'mpvpaper' 'ffmpeg' 'libnotify' 'jq')
makedepends=('cargo')
optdepends=(
    'electron: for desktop web streams & HTML widget overlay engine'
    'hyprland: for automatic workspace IPC event sync'
    'sway: for sway/i3 workspace IPC event sync'
)
source=("${pkgname}-${pkgver}.tar.gz::https://github.com/misternegative21/omywall/archive/refs/tags/v${pkgver}.tar.gz")
sha256sums=('SKIP')

build() {
    cargo build --release
}

package() {
    install -Dm755 "target/release/${pkgname}" "${pkgdir}/usr/bin/${pkgname}"
    install -Dm644 "omywall.desktop" "${pkgdir}/usr/share/applications/${pkgname}.desktop"
    install -Dm644 "assets/omarchy-wall.svg" "${pkgdir}/usr/share/icons/hicolor/scalable/apps/${pkgname}.svg"
    install -Dm644 "README.md" "${pkgdir}/usr/share/doc/${pkgname}/README.md"
}
