# Local source snapshot package. Build from the project root with:
#   mkdir -p .makepkg/{src,pkg}
#   SRCDEST="$PWD/.makepkg/src" BUILDDIR="$PWD/.makepkg/pkg" makepkg -f
# Replace this local source=() approach with a versioned release tarball and
# a verified checksum when an upstream release URL is available.

pkgname=oom-alerter
pkgver=0.1.0
pkgrel=1
pkgdesc='Desktop notification daemon for pre-OOM memory pressure on Linux'
arch=('x86_64')
license=('MIT')
depends=('dbus')
makedepends=('rust')
source=()
sha256sums=()

prepare() {
  local source_tree="$srcdir/$pkgname-$pkgver"

  rm -rf "$source_tree"
  mkdir -p "$source_tree"
  for entry in "$startdir"/* "$startdir"/.[!.]*; do
    case "${entry##*/}" in
      .devenv|.makepkg|target)
        continue
        ;;
    esac
    cp -a "$entry" "$source_tree"/
  done
}

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
