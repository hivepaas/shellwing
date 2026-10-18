# shellwing

A fast, native SSH client for macOS, Windows and Linux.

Built in Rust on [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui),
with [libghostty-vt](https://ghostty.org) for terminal emulation and
[russh](https://github.com/Eugeny/russh) for SSH.

Status: early development.

## Build

Needs Rust 1.95+ and [Zig](https://ziglang.org/) 0.16 on `PATH` (libghostty-vt is
built from source).

```sh
cargo build --release
```

On Linux both Wayland and X11 are enabled; without the X11 development
libraries, build with `--no-default-features --features wayland`. Per-platform
setup, Windows VMs, Vietnamese input and a manual test checklist are in
[docs/platforms.md](docs/platforms.md). CI builds Linux on every push and macOS
and Windows when started by hand, and uploads the binaries as artifacts.

## Run

```sh
shellwing [user@]host[:port]
```

Authentication tries the ssh-agent keys (`$SSH_AUTH_SOCK`; on Windows the OpenSSH
agent, then Pageant), then the password in
`$SHELLWING_PASSWORD`. Host keys are checked against `~/.ssh/known_hosts`
(override with `$SHELLWING_KNOWN_HOSTS`): a new host is recorded on first use and
a changed key refuses the connection.

## Layout

| Crate | Role |
| --- | --- |
| `crates/term` | libghostty-vt on its own thread; produces immutable frames |
| `crates/ssh` | russh sessions, OpenSSH-compatible host key checks, agent/password auth |
| `crates/app` | GPUI window: rendering, keys, IME (preedit), window chrome |

`SHELLWING_BENCH=latency` or `flood` runs a measurement and prints a `REPORT` line.
