<div align="center">

<img src="packaging/icons/shellwing.svg" alt="shellwing logo" width="96" height="96">

# shellwing

**A fast, native SSH client for macOS, Windows and Linux.**

Tabs, split panes, SFTP, port forwarding and an encrypted vault that syncs between your devices, in a GPU-rendered terminal built in Rust.

[![CI](https://github.com/hivepaas/shellwing/actions/workflows/ci.yml/badge.svg)](https://github.com/hivepaas/shellwing/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/Rust-1.95+-DEA584?style=flat&logo=rust)](https://www.rust-lang.org)
[![GPUI](https://img.shields.io/badge/UI-GPUI-7B61FF?style=flat)](https://github.com/zed-industries/zed/tree/main/crates/gpui)
[![libghostty](https://img.shields.io/badge/Terminal-libghostty-3551F3?style=flat)](https://ghostty.org)
[![Platforms](https://img.shields.io/badge/Platforms-macOS%20%7C%20Windows%20%7C%20Linux-555?style=flat)](docs/platforms.md)
[![License](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)

[Features](#-key-features) • [Getting Started](#-getting-started) • [Configuration](#-configuration) • [Architecture](#-architecture) • [License](#-license)

</div>

---

## 🌟 Key Features

* **Native and fast:** a GPU-rendered terminal on [libghostty-vt](https://ghostty.org), parsing on its own thread; the display's full refresh rate under heavy output, sharp text at any HiDPI scale, and input methods (IME) with the composing text at the cursor.
* **SSH the way OpenSSH does it:** `~/.ssh/config` (aliases, `ProxyJump`, forwards, identity files), `known_hosts` checks, agent keys, key files, certificates, keyboard-interactive and two-factor codes, and agent forwarding.
* **Tabs and panes:** split a tab to the same host, move between panes, type into every pane at once, reorder tabs by dragging; tabs follow the shell's title.
* **Files over SFTP:** browse, upload and download (drag and drop too), rename, delete, and edit remote files in your own editor.
* **Port forwarding:** local, remote and SOCKS (`-L`, `-R`, `-D`), from the command line, a saved host or the palette while connected.
* **Saved hosts:** `hosts.toml` with groups, colors, favorites, tags, jump hosts, forwards and a command to run after connecting; a host list and a palette to find and open them.
* **Vault:** passwords, SSH keys, snippets, environments, config files and notes, encrypted and unlocked through the system keychain, Windows Hello or Touch ID, or a passphrase.
* **Sync between devices:** the vault, hosts and settings through a synced folder (iCloud Drive, Dropbox, Syncthing...), WebDAV, S3-compatible storage or a Git repository, end-to-end encrypted, with a recovery key and device pairing.
* **Made for work:** a command palette, find in the terminal and its history, session logs, a local terminal beside the SSH tabs, customizable shortcuts, light and dark themes (gradients too), and an interface in 10 languages.
* **Reconnects on its own:** a lost connection is retried with a back-off, and forwards come back with it.

---

## 🚀 Getting Started

Download the latest release from [GitHub Releases](https://github.com/hivepaas/shellwing/releases/latest):

| System | File |
| --- | --- |
| macOS (Apple silicon, Intel) | `shellwing_<version>_macos_<arch>.dmg` |
| Windows | `shellwing_<version>_windows_x86_64-setup.exe`, or the `.zip` |
| Linux (x86_64, aarch64) | the `.AppImage`, the `.deb`, or the `.tar.gz` |

The releases are not signed with an Apple Developer ID or a Windows certificate yet: the first time, macOS asks you to allow it in *System Settings › Privacy & Security* (**Open Anyway**), and Windows' SmartScreen under **More info › Run anyway**. The app tells you when a newer version is out, and installs it when you say so.

### Building from source

It needs Rust 1.95+ and [Zig](https://ziglang.org/) 0.16 on `PATH` (libghostty-vt is built from source):

```sh
make build                    # a release build for this computer, into dist/
make package                  # and the packages a release ships (docs/releasing.md)
cargo build --release         # or by hand
```

Then:

```sh
shellwing                     # the window with the host list
shellwing [user@]host[:port]  # and a tab connected to the host
shellwing --local             # and a tab with a shell on this computer
```

`host` can be a name from `hosts.toml` or an alias from `~/.ssh/config`. Per-platform setup (toolchains, Linux libraries, testing on Windows in a VM) is in [docs/platforms.md](docs/platforms.md).

---

## ⚙️ Configuration

Settings, hosts and themes are TOML files in the config directory (`~/.config/shellwing` on Linux) and apply while the app runs; *Settings* in the app edits the same files. Everything is described in [docs/configuration.md](docs/configuration.md), and the design notes are in [docs/design](docs/design/README.md).

---

## 🏗️ Architecture

The app is a [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) window over a core with no UI, which also builds for iOS and Android.

| Crate | Role |
| --- | --- |
| `crates/app` | The GPUI window: tabs, panes, host list, palette, panels, settings |
| `crates/core` | Sessions without a UI: connecting and reconnecting, prompts, saved secrets |
| `crates/term` | Terminal emulation: libghostty-vt on its own thread, producing frames to paint |
| `crates/ssh` | SSH over [russh](https://github.com/Eugeny/russh), as OpenSSH does it |
| `crates/vault` | The encrypted vault, and its sync |
| `crates/remote` | Sync backends over the network |
| `crates/config` | `config.toml`, `hosts.toml` and themes |
| `crates/presence` | Windows Hello and Touch ID |
| `crates/i18n` | The interface's languages |
| `crates/note-editor` | The notes' editor |

---

## 📜 License

shellwing is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License, version 3 or (at your option) any later version. See [LICENSE](LICENSE). Copyright (C) 2026 HivePaaS.

The name and logo are not covered by the GPL: see [TRADEMARKS.md](TRADEMARKS.md). Contributions are welcome under the terms in [CONTRIBUTING.md](CONTRIBUTING.md).

Code adapted from other projects keeps its license and notice: `crates/app/src/input.rs` is adapted from GPUI's `examples/input.rs` (Apache-2.0, Zed Industries), `crates/note-editor` from zorite-editor (MIT; see `crates/note-editor/LICENSE`), and most built-in terminal themes come from iTerm2-Color-Schemes (MIT; see `crates/config/themes/LICENSE-iTerm2-Color-Schemes`). The libraries shellwing builds on are all under licenses that allow this (MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0 and similar).
