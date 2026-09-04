# oom-alerter

Small foreground Linux daemon that samples `/proc` once per second and sends desktop notifications before memory pressure becomes an OOM event.

## Arch Linux package

`PKGBUILD` packages a local source snapshot because this project does not yet
have an upstream release URL. From the project root, build the package without
placing makepkg's `src/` directory alongside the Rust `src/` directory:

```sh
mkdir -p .makepkg/{src,pkg}
SRCDEST="$PWD/.makepkg/src" BUILDDIR="$PWD/.makepkg/pkg" makepkg -f
```

Install the generated package with `pacman -U oom-alerter-0.1.0-1-x86_64.pkg.tar.*`
as root. Uninstall it with `pacman -Rns oom-alerter`. The package installs the
binary at `/usr/bin/oom-alerter`, the user service at
`/usr/lib/systemd/user/oom-alerter.service`, the README, and the MIT license.
It does not enable the service automatically.

After installation, enable it for the current user:

```sh
systemctl --user daemon-reload
systemctl --user enable --now oom-alerter
systemctl --user status oom-alerter
journalctl --user -u oom-alerter
```

Notifications require a desktop notification server on the session D-Bus; the
daemon does not assume that a particular compositor or window manager, including
Niri, provides one. Stop and disable the user service before uninstalling when
it is no longer wanted:

```sh
systemctl --user disable --now oom-alerter
```

When a tagged upstream release is published, replace the PKGBUILD's empty
`source=()` and `sha256sums=()` with the release tarball URL and its verified
checksum, then build from that release source rather than this working tree.

## Manual source installation

Build and install the binary and matching packaged-path unit locally:

```sh
cargo build --release
sudo install -Dm755 target/release/oom-alerter /usr/local/bin/oom-alerter
sudo install -Dm644 systemd/oom-alerter.service /etc/systemd/user/oom-alerter.service
sudo sed -i 's|/usr/bin/oom-alerter|/usr/local/bin/oom-alerter|' \
  /etc/systemd/user/oom-alerter.service
systemctl --user daemon-reload
systemctl --user enable --now oom-alerter
```

`--once` is a snapshot diagnostic: it reads and prints one sample, then exits without waiting or alerting. The sampling `--interval` is configurable in seconds and defaults to 1 second. Memory thresholds accept plain bytes for compatibility or binary sizes with `K`, `M`, `G` (and `KiB`, `MiB`, `GiB`) suffixes, case-insensitively; for example, `--warning 20G --critical 768M --hysteresis 3GiB`. Other options are `--warning` (default 3 GiB), `--critical` (768 MiB), `--hysteresis` (256 MiB), `--dwell` (10 seconds), `--warning-repeat` (60 seconds), and `--critical-repeat` (15 seconds). Swap/zram usage is diagnostic context only and never triggers alerts. PSI `some` and `full` stall rates contribute to severity, while unavailable PSI falls back to memory availability and its downward slope. The daemon only observes and notifies; it never kills processes and has no config file. Notification updates may depend on the desktop server's D-Bus support; failures are logged nonfatally.
