# Development guide

## Project scope

`oom-alerter` is a small, Linux-only Rust foreground daemon. It samples
`/proc` and Linux PSI once per interval and sends desktop notifications before
memory pressure becomes an OOM event. Keep the implementation observational:
it must never kill, stop, or otherwise manage processes. Configuration is
CLI-only; do not add a configuration file or hidden environment-based policy.
Swap and zram are diagnostic context, not alert triggers. PSI `some` and
`full` pressure contribute to severity, with memory availability and its
downward slope as the fallback when PSI is unavailable. Notification failures
are nonfatal and should be logged.

## Development environment

The repository uses devenv and enables Rust in `devenv.nix`. Enter the project
environment with:

```sh
devenv shell
```

Run Cargo commands from that shell so the pinned development toolchain and
dependencies are used. Do not edit generated `.devenv` files; change
`devenv.nix`, `devenv.yaml`, or other tracked inputs instead.

## Build and validation

Before submitting changes, format, compile, and run the test suite:

```sh
cargo fmt -- --check
cargo check --locked
cargo test --locked
cargo clippy --locked -- -D warnings
cargo build --release --locked
```

Use `cargo fmt` to apply formatting when needed. Keep warnings addressed where
practical and avoid adding dependencies for functionality already available in
the standard library or existing crates.

## Running locally

Run the daemon in the foreground during development:

```sh
cargo run -- [OPTIONS]
```

For a single diagnostic snapshot, use `--once`. It reads and prints one sample,
then exits without waiting or alerting:

```sh
cargo run -- --once
```

The interval and alert policy are command-line options. Preserve the existing
defaults and semantics unless a change explicitly requires otherwise. Defaults
are:
- MemAvailable warning: 3 GiB (`--warning 3GiB`)
- MemAvailable critical: 768 MiB (`--critical 768MiB`)
- Hysteresis: 256 MiB (`--hysteresis 256MiB`)
- Dwell: 10 seconds (`--dwell 10`)
- PSI some warning: 10% sustained over dwell (`--psi-some-warning 10`)
- PSI full critical: 5% sustained over dwell (`--psi-full-critical 5`)
- Decline warning rate: 1 GiB/min below 6 GiB MemAvailable (`--decline-warning 1GiB --decline-warning-gate 6GiB`)
- Decline critical rate: 2 GiB/min below 4 GiB MemAvailable (`--decline-critical 2GiB --decline-critical-gate 4GiB`)
- Warning reminder repeat: 5 minutes / 300 seconds (`--warning-repeat 300`)
- Critical reminder repeat: 60 seconds (`--critical-repeat 60`)

Warning to Critical transition escalates immediately without dwell delay.
Recovery requires all active triggers to clear, including half-threshold exit
hysteresis for PSI metrics.

A desktop notification server on the session D-Bus is required for notifications;
the application must not assume that a particular compositor or window manager
provides one.

## Systemd user service workflow

The checked-in unit is `systemd/oom-alerter.service`. Build and install the
binary and unit for a user session with:

```sh
cargo build --release
install -Dm755 target/release/oom-alerter ~/.local/bin/oom-alerter
install -Dm644 systemd/oom-alerter.service ~/.config/systemd/user/oom-alerter.service
systemctl --user daemon-reload
systemctl --user enable --now oom-alerter
```

Inspect status and logs with:

```sh
systemctl --user status oom-alerter
journalctl --user -u oom-alerter
```

After unit changes, run `systemctl --user daemon-reload` and restart the
service. The unit targets the graphical session and uses the user session
D-Bus; do not convert it into a system-wide service without an explicit design
change.

## Arch Linux packaging

`PKGBUILD` consumes the tagged GitHub release asset at
`https://github.com/jonjomckay/oom-alerter/releases/download/v${pkgver}/oom-alerter-${pkgver}.tar.gz`.
For each release, update `pkgver` and its verified `sha256sums` entry together
from the published `.sha256` asset, then validate it with:

```sh
makepkg -si
```

The package must build with Cargo's lockfile, run tests in `check()`, install
the binary to `/usr/bin`, and install the user unit to
`/usr/lib/systemd/user`. It must not enable the service. Before submitting
packaging changes, inspect the package contents with `pacman -Qlp` and run
`namcap` when it is available. Do not use `SKIP` checksums.

## Release workflow

Tags named `v<version>` run the release workflow, which requires the tag to
match the Cargo package version, runs the locked CI checks, creates a
deterministic source archive from the tag's Git tree, writes its SHA-256 file,
and publishes both assets using the repository token. The archive is rooted at
`oom-alerter-<version>/` and excludes packaging, CI, and developer-only files
through `.gitattributes`; it must retain `Cargo.toml`, `Cargo.lock`, `src`, the
systemd unit, README, and LICENSE. Create and verify the release before
updating the PKGBUILD checksum for that version.

## Code conventions

- Keep modules focused: sampling and procfs/PSI parsing belong in the memory
  layer, alert transitions and policy belong in the policy layer, and desktop
  notification integration belongs in the notify layer.
- Prefer small, testable pure functions for parsing, threshold calculations,
  hysteresis, dwell, and repeat timing.
- Treat missing or malformed procfs/PSI data defensively and preserve useful
  diagnostics without making transient observation failures fatal.
- Keep CLI help and option behavior clear and consistent with the existing
  command-line interface.
- Make the smallest coherent change; update tests and documentation when
  behavior changes, but do not modify generated build or devenv artifacts.
