# Update pkgver and sha256sums together after each tagged release.
pkgname=oom-alerter
pkgver=0.1.0
pkgrel=1
pkgdesc='Desktop notification daemon for pre-OOM memory pressure on Linux'
arch=('x86_64')
url='https://github.com/jonjomckay/oom-alerter'
license=('MIT')
depends=('dbus')
makedepends=('rust')
source=("$pkgname-$pkgver.tar.gz::https://github.com/jonjomckay/oom-alerter/releases/download/v${pkgver}/oom-alerter-${pkgver}.tar.gz")
sha256sums=('a049fbcbe8aeb2adb2ecebcbd815eee108fe2c3edb2809bb811a0999ca8a4494')

build() {
  cd "$srcdir/$pkgname-$pkgver"
  cargo build --release --locked
}

check() {
  cd "$srcdir/$pkgname-$pkgver"
  cargo test --locked
}

package() {
  cd "$srcdir/$pkgname-$pkgver"

  install -Dm755 "target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
  install -Dm644 "systemd/$pkgname.service" \
    "$pkgdir/usr/lib/systemd/user/$pkgname.service"
  install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
