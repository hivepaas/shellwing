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

On Linux, `scripts/install-desktop.sh` adds the app's icon and a launcher for
the built binary to your desktop (the dock, Alt+Tab and the app grid then show
it); `--remove` takes them away. The icon itself is `packaging/icons/shellwing.svg`.

## Run

```sh
shellwing                     # the window with the host list
shellwing [user@]host[:port]  # and a tab connected to host
```

`host` can be a name from `hosts.toml` or an alias from `~/.ssh/config`
(`HostName`, `User`, `Port`, `IdentityFile` and `ProxyJump` are honoured). The host
list on the left shows both; clicking one opens a tab. Sign-in follows OpenSSH:
agent keys, key files, keyboard-interactive (one-time codes, or the password when
PAM asks for it), then a password;
questions are asked in the terminal. Host keys are checked against
`~/.ssh/known_hosts` (override with `$SHELLWING_KNOWN_HOSTS`): a new key is shown
with its fingerprint for you to accept, a changed key refuses the connection.

## Shortcuts

Ctrl+Shift on Linux and Windows is Cmd on macOS.

| Keys | Does |
| --- | --- |
| Ctrl+Shift+K (Cmd+K) | Palette: find a host, connect to `user@host`, change the theme, run an action |
| Ctrl+Shift+T | A new tab (opens the palette) |
| Ctrl+Shift+D / E (Cmd+D / Cmd+Shift+D) | Split right / down, to the same host |
| Alt+arrows (Cmd+Alt+arrows) | Move between panes (with one pane they go to the shell) |
| Ctrl+Shift+W | Close the pane; the last one closes the tab |
| Cmd+Q (macOS) | Quit; closing the window quits too, asking first while connections are open |
| Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+PageDown / PageUp | Next / previous tab (also Cmd+Shift+] / [ on macOS) |
| Alt+1…9 (Cmd+1…9) | Go to a tab |
| Ctrl+Shift+B | Show or hide the host list |
| Ctrl+Shift+C / V, Shift+Insert | Copy / paste |
| Shift+PageUp / PageDown / End | Scroll the history / back to the bottom |
| Ctrl+= / Ctrl+- / Ctrl+0 | Zoom the font in this tab (Cmd on macOS) |
| Ctrl+Shift+] / [ (Cmd+] / [) | Next / previous theme, saved to `config.toml` |
| Ctrl+click (Cmd+click) | Open a link |

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
| `crates/vault` | Saved passwords and passphrases: encrypted records in SQLite, key in the OS keychain |
| `crates/config` | `config.toml`, `hosts.toml` and themes: loading, precedence, comment-preserving writes |
| `crates/app` | GPUI window: tabs, host list, terminal rendering, keys, IME (preedit), window chrome |

`SHELLWING_BENCH=latency` or `flood` runs a measurement and prints a `REPORT` line.

## License

shellwing is free software: you can redistribute it and/or modify it under the
terms of the GNU General Public License, version 3 or (at your option) any later
version. See [LICENSE](LICENSE). Copyright (C) 2026 HivePaaS.

The name and logo are not covered by the GPL: see [TRADEMARKS.md](TRADEMARKS.md).
Contributions are welcome under the terms in [CONTRIBUTING.md](CONTRIBUTING.md).

Code adapted from other projects keeps its license and notice:
`crates/app/src/input.rs` is adapted from GPUI's `examples/input.rs`
(Apache-2.0, Zed Industries). The libraries shellwing builds on are all under
licenses that allow this (MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0 and similar).
