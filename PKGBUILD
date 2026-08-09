# Maintainer: Nikas <n3koBomb>
pkgname=anixart-arch
pkgver=0.4.1
pkgrel=1
pkgdesc='Unofficial native Linux desktop client for Anixart'
arch=('x86_64' 'aarch64')
url='https://github.com/n3koBomb/anixart-arch'
license=('GPL-3.0-or-later')
depends=('gtk4' 'libadwaita' 'webkitgtk-6.0' 'mpv' 'gstreamer' 'gst-plugins-good' 'gst-plugins-bad' 'gst-libav' 'gst-plugin-va')
makedepends=('cargo')
optdepends=('yt-dlp: expand mpv support for compatible non-direct URLs'
            'gst-plugins-ugly: additional GStreamer codecs for unusual provider media'
            'intel-media-driver: VA-API backend for modern Intel graphics including Arc'
            'libva-mesa-driver: VA-API backend for AMD/Mesa graphics'
            'libva-utils: vainfo command for hardware video diagnostics')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('8e5f7791610dd5f4e3d3c0fb5fef14d68b1847146aec48feb37f7acba1ae647d')

_ring_lto_compat() {
  # ring contains C/ASM objects. Arch/CachyOS LTO + lld can otherwise leave
  # ring_core_* symbols unresolved at the final Rust link stage.
  export CFLAGS="${CFLAGS} -ffat-lto-objects"
  export CXXFLAGS="${CXXFLAGS} -ffat-lto-objects"
}

build() {
  cd "$pkgname-$pkgver"
  _ring_lto_compat
  cargo build --release --locked
}

check() {
  cd "$pkgname-$pkgver"
  _ring_lto_compat
  cargo test --release --locked
}

package() {
  cd "$pkgname-$pkgver"
  install -Dm755 target/release/anixart "$pkgdir/usr/bin/anixart"
  install -Dm644 data/anixart-arch.png "$pkgdir/usr/share/icons/hicolor/512x512/apps/anixart-arch.png"
  install -Dm644 data/io.github.anixartarch.AnixartArch.desktop "$pkgdir/usr/share/applications/io.github.anixartarch.AnixartArch.desktop"
  install -Dm644 data/io.github.anixartarch.AnixartArch.metainfo.xml "$pkgdir/usr/share/metainfo/io.github.anixartarch.AnixartArch.metainfo.xml"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
