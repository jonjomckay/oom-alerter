# Update pkgver and sha256sums together after each tagged release.
pkgname=oom-alerter
pkgver=0.1.1
pkgrel=1
pkgdesc='Desktop notification daemon for pre-OOM memory pressure on Linux'
arch=('x86_64')
url='https://github.com/jonjomckay/oom-alerter'
license=('MIT')
depends=('dbus')
makedepends=('rust')
source=("$pkgname-$pkgver.tar.gz::https://github.com/jonjomckay/oom-alerter/releases/download/v${pkgver}/oom-alerter-${pkgver}.tar.gz")
sha256sums=('8c6e6d97be42716666ca8d0c30ec7f553301268ef314ec679827ab570188027b')

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
