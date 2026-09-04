# oom-alerter

Small foreground Linux daemon that samples `/proc` once per second and sends desktop notifications before memory pressure becomes an OOM event.

## Arch Linux package

`PKGBUILD` packages tagged GitHub release assets. The initial `0.1.0` asset has
not been published yet, so its checksum is deliberately an invalid placeholder.
After each tagged release, update `pkgver` and replace `sha256sums` with the
SHA-256 value from `oom-alerter-$pkgver.tar.gz.sha256`. Then build and install
with:

```sh
makepkg -si
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

The package builds and tests with Cargo's lockfile. Validate package changes
with `makepkg -si`, inspect package contents with `pacman -Qlp`, and run
`namcap` when it is available. The package does not enable the service.

## Releases

Pushing a tag named `v<version>` triggers the release workflow. It requires the
tag version to match `Cargo.toml`, runs the full locked CI suite, creates a
deterministic `oom-alerter-<version>.tar.gz` source archive and SHA-256 file,
then publishes both as GitHub Release assets. The archive contains the Rust
package files, `systemd/oom-alerter.service`, README, and license; development
and packaging infrastructure is excluded.

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
