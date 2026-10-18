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
[docs/platforms.md](docs/platforms.md). CI builds Linux, macOS and Windows and
uploads the binaries as artifacts.

## Run

```sh
shellwing [user@]host[:port]
```

`host` can be a name from `hosts.toml` or an alias from `~/.ssh/config`
(`HostName`, `User`, `Port`, `IdentityFile`, `ProxyJump` are honoured). Sign-in
follows OpenSSH: agent keys, key files, keyboard-interactive (one-time codes),
then a password; questions are asked in the terminal. Host keys are checked
against `~/.ssh/known_hosts` (override with `$SHELLWING_KNOWN_HOSTS`): a new key
is shown with its fingerprint for you to accept, a changed key refuses the
connection.

## Configuration

Settings (theme, font, cursor, scrollback), hosts and custom themes are TOML
files in the config directory (`~/.config/shellwing` on Linux) and apply while
running. 19 themes are built in, 12 of them gradients; Ctrl+Shift+] and
Ctrl+Shift+[ (Cmd+] and Cmd+[ on macOS) step through them. Details:
[docs/configuration.md](docs/configuration.md).

## Layout

| Crate | Role |
| --- | --- |
| `crates/term` | libghostty-vt on its own thread; produces immutable frames |
| `crates/ssh` | russh sessions as OpenSSH does them: ssh config, host keys, sign-in, ProxyJump |
| `crates/config` | `config.toml`, `hosts.toml` and themes: loading, precedence, comment-preserving writes |
| `crates/app` | GPUI window: rendering, keys, IME (preedit), window chrome |

`SHELLWING_BENCH=latency` or `flood` runs a measurement and prints a `REPORT` line.
