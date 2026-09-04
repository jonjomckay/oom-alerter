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
cargo check
cargo test
cargo clippy -- -D warnings
cargo build --release
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
defaults and semantics unless a change explicitly requires otherwise. A
desktop notification server on the session D-Bus is required for notifications;
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

`PKGBUILD` is intentionally a local-source-snapshot package until an upstream
release URL exists. Run `makepkg` with its source and build directories outside
the project root because makepkg's default `src/` directory conflicts with the
Rust source directory:

```sh
mkdir -p .makepkg/{src,pkg}
SRCDEST="$PWD/.makepkg/src" BUILDDIR="$PWD/.makepkg/pkg" makepkg -f
```

The package must build with Cargo's lockfile, run tests in `check()`, install
the binary to `/usr/bin`, and install the user unit to
`/usr/lib/systemd/user`. It must not enable the service. Before submitting
packaging changes, inspect the package contents with `pacman -Qlp` and run
`namcap` when it is available. When tagged releases become available, replace
the local-source PKGBUILD strategy with a versioned release tarball and a
verified checksum.

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
