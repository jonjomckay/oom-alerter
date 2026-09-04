# oom-alerter

Small foreground Linux daemon that samples `/proc` once per second and sends desktop notifications before memory pressure becomes an OOM event.

## Arch Linux package

`PKGBUILD` packages tagged GitHub release assets. After each tagged release,
update `pkgver` and its verified `sha256sums` entry from the published
`oom-alerter-$pkgver.tar.gz.sha256` asset. Then build and install with:

```sh
makepkg -si
```

Install the generated package with `pacman -U oom-alerter-0.1.1-1-x86_64.pkg.tar.*`
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

`--once` is a snapshot diagnostic: it reads and prints one sample, explicitly indicating that rate deltas are unavailable for single-shot reads, then exits without waiting or alerting. `--verbose` (`-v`) provides foreground diagnostic logging of every sample and state evaluation without notification spam.

The sampling `--interval` is configurable in seconds and defaults to 1 second. Memory thresholds and rates accept plain bytes for compatibility or binary sizes with `K`, `M`, `G` (and `KiB`, `MiB`, `GiB`) suffixes, case-insensitively; for example, `--warning 20G --critical 768M --hysteresis 3GiB`.

Available alert policy options and defaults:
- `--warning`: MemAvailable warning threshold (default: 3 GiB).
- `--critical`: MemAvailable critical threshold (default: 768 MiB).
- `--hysteresis`: Additional MemAvailable above warning threshold required for recovery to Normal (default: 256 MiB).
- `--dwell`: Dwell time in seconds required before state transitions (default: 10 seconds; Warning-to-Critical escalation is immediate).
- `--psi-some-warning`: PSI `some` stall percentage for Warning alert sustained over dwell (default: 10%, accepts e.g. `10` or `10%`).
- `--psi-full-critical`: PSI `full` stall percentage for Critical alert sustained over dwell (default: 5%, accepts e.g. `5` or `5%`).
- `--decline-warning`: Rapid decline rate for Warning alert (default: 1 GiB/min, specified as `1GiB`).
- `--decline-warning-gate`: MemAvailable gate below which Warning decline rate triggers alert (default: 6 GiB).
- `--decline-critical`: Rapid decline rate for Critical alert (default: 2 GiB/min, specified as `2GiB`).
- `--decline-critical-gate`: MemAvailable gate below which Critical decline rate triggers alert (default: 4 GiB).
- `--warning-repeat`: Reminder interval in seconds while in Warning state (default: 300 seconds / 5 minutes).
- `--critical-repeat`: Reminder interval in seconds while in Critical state (default: 60 seconds).

Notifications and transition logs include the specific trigger reason (e.g. `PSI some 12.4%`, `rapid decline: 1200 MiB/min`, `MemAvailable low: 2500 MiB`), current MemAvailable, decline slope in MiB/min, and PSI stall percentages when available. Swap/zram usage is diagnostic context only and never triggers alerts. If PSI is not supported by the kernel, PSI fields are reported as unavailable (`n/a`) rather than zero, and the alerter falls back to memory availability and decline slope. Recovery requires all active alert triggers to clear, including sensible PSI exit hysteresis (half entry threshold) and slope gates. The daemon only observes and notifies; it never kills processes and has no config file. Notification updates may depend on the desktop server's D-Bus support; failures are logged nonfatally.
