# Maintainer: Ardeaf <ardeaf@gmail.com>

pkgname=redelete
pkgver=0.2.1
pkgrel=1
pkgdesc='Delete all of your reddit comments and submissions, with optional filters to skip certain posts.'
arch=('x86_64')
url=https://github.com/ardeaf/redelete
license=('MIT' 'APACHE')
depends=('gcc-libs')
makedepends=('rust')
source=("redelete-$pkgver.tar.gz::$url/archive/v$pkgver.tar.gz")

build() {
  cd redelete-$pkgver
  cargo build --release --locked
}

check() {
  cd redelete-$pkgver
  cargo test --release
}

package() {
  cd redelete-$pkgver
  install -Dm755 target/release/fd "$pkgdir"/usr/bin/redelete
  install -Dm644 LICENSE "$pkgdir"/usr/share/licenses/redelete/LICENSE
}

