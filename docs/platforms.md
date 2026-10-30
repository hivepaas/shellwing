# Building and testing on macOS, Windows and Linux

CI builds and tests every push to main and every pull request into main on Linux
(x86_64); pull requests stacked on other branches run once retargeted to main. macOS
(arm64) and Windows (x86_64) run when started by hand, once a feature is ready:
Actions → CI → Run workflow, pick the branch and the platforms, or
`gh workflow run ci.yml --ref <branch> -f platforms=all`. Each run uploads the
binaries as workflow artifacts (`shellwing-<platform>`), so a test machine does
not need a toolchain: download the artifact from the run's summary page.

Rendering and input (IME, keyboard layouts, HiDPI) still need a real desktop;
CI only proves that the code builds and the tests pass.

## Against real OpenSSH servers

`make e2e-openssh` (`scripts/e2e_openssh.sh`) runs `crates/ssh/tests/openssh.rs`
against real OpenSSH servers in Docker, set up the way an organization sets
them up:

- A bastion is the only server this machine reaches.
- Behind it, on a network only the bastion is on, are Alpine (OpenSSH 10),
  Debian 12 (9.2), Ubuntu 20.04 (8.2) and Rocky 8 (8.0).
- Hosts are trusted through a host CA (`@cert-authority`).
- A user CA, three kinds of key and a password are made for each run.

The tests sign in each way and go through ProxyJump and ProxyCommand. They
also cover SFTP, a local forward, agent forwarding and the shell's exit
status. CI runs them on every pull request. Locally they need Docker and
`ssh-keygen`. `SHELLWING_E2E_KEEP=1` leaves the servers running to look at,
and `scripts/e2e_openssh.sh -- <filter>` runs some of the tests. Without the
script's environment, `cargo test` skips them.

## Toolchain (to build locally)

All platforms need Rust (via [rustup](https://rustup.rs)) and **Zig 0.17**
from [ziglang.org/download](https://ziglang.org/download/) on `PATH`;
libghostty-vt is compiled from source with it. rustup installs the Rust version in
`rust-toolchain.toml` by itself, the first time cargo runs in the repository.

| Platform | Also install |
| --- | --- |
| macOS | Xcode Command Line Tools: `xcode-select --install` |
| Windows | Visual Studio Build Tools with the "Desktop development with C++" workload (MSVC and the Windows SDK) |
| Debian/Ubuntu | `sudo apt-get install build-essential pkg-config libfontconfig-dev libfreetype-dev libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libx11-xcb-dev libxcb1-dev libxcb-xkb-dev libxcb-shape0-dev libxcb-xfixes0-dev libvulkan-dev libdbus-1-dev libzstd-dev libpam0g-dev` |

`make build` builds a release for this computer into `dist/`; `make` alone lists
the other targets (`make run`, `make check`, `make doctor`, the phone cores). By
hand:

```sh
cargo build --release          # Linux: Wayland and X11
cargo build --release -p shellwing --no-default-features --features wayland   # Linux without the X11 libraries
```

Downloaded macOS binaries are quarantined by Gatekeeper; clear the flag once:

```sh
xattr -d com.apple.quarantine ./shellwing && chmod +x ./shellwing
```

## The Rust version

`rust-toolchain.toml` pins the Rust that local builds, CI and releases use, and
`rust-version` in `Cargo.toml` follows it, so a new Rust never turns CI red on
its own. Rust ships every six weeks; take every second release (about every
twelve weeks), or one sooner when a fix or a feature in it is needed, once it
is two weeks old (its point release, if any, is out by then). In a pull request
of its own:

1. Set `channel` in `rust-toolchain.toml` and `rust-version` in `Cargo.toml`.
2. Fix what the new clippy finds: `make check`.
3. Run CI and the UI tests on every platform:
   `gh workflow run ci.yml --ref <branch> -f platforms=all` and
   `gh workflow run ui-tests.yml --ref <branch> -f platforms=all`.

Targets added by hand (`rustup target add`, for the phones) belong to one
version: add them again after a raise. The `make` targets for the phones do.

## The phone apps

The iPhone app is in `ios/` (its README says how to build it), the Android app
in `android/` (`make android`; its README). Both are UI over `crates/mobile`,
which wraps `shellwing-core` (term, ssh, config, vault) for Swift and Kotlin
with UniFFI. The vault's key store comes from each app (`PlatformKeys`): the
keyring crate picks none on iOS or Android.

To build the core alone for Android, with the NDK (`ANDROID_NDK_HOME`) and its
clang for the C parts:

```sh
bin="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin"  # linux-x86_64 on Linux
export CC_aarch64_linux_android="$bin/aarch64-linux-android24-clang"
export AR_aarch64_linux_android="$bin/llvm-ar"
export RANLIB_aarch64_linux_android="$bin/llvm-ranlib"   # OpenSSL, for libgit2
rustup target add aarch64-linux-android
cargo build -p shellwing-mobile --target aarch64-linux-android
```

CI checks the Android core and bindings on every pull request; the Android and
iOS apps when started by hand (Actions > CI > Run workflow, `android`, `ios`
or `all`).

## Windows in a virtual machine

GPUI renders with Direct3D 11 on Windows, so the VM's 3D support decides
whether rendering is fast:

| Host | VM | Notes |
| --- | --- | --- |
| Apple silicon Mac | VMware Fusion (free for personal use) or Parallels, Windows 11 ARM | Good Direct3D 11. Windows 11 ARM runs the x86_64 artifact through its built-in emulation. |
| Linux | VMware Workstation (free for personal use) | Usable 3D acceleration. |
| Linux | QEMU/KVM or VirtualBox | Little or no Direct3D; Windows falls back to software rendering (WARP). Fine for checking behaviour, not for measuring speed. |

Microsoft's Windows 11 Enterprise evaluation image works for testing.

## What to check by hand

Connect to any SSH server you can reach (`shellwing user@host`), with the key in
your ssh-agent (OpenSSH agent or Pageant on Windows) or, in a debug build,
`SHELLWING_PASSWORD`.

1. Typing, Enter, Backspace, arrows, Ctrl+C; `htop` and `vim` (including the mouse).
2. Input methods: with an input source that composes text (an IME), the
   composing text shows at the cursor and the committed text reaches the shell.
3. Resize the window; the remote program follows.
4. Scrollback with the wheel or trackpad, the "Back to bottom" badge, Shift+PageUp.
5. Select with the mouse, copy (Ctrl+Shift+C, Cmd+C on macOS), paste
   (Ctrl+Shift+V or Shift+Insert, Cmd+V); a multi-line paste at a prompt is refused
   once, then sent when pasted again.
6. Links: hold Ctrl (Cmd on macOS) over a URL, click to open.
7. `echo "日本語 한국어 😀 🇻🇳"` stays aligned on the grid.
8. HiDPI: text is sharp at the display's scale (e.g. 125 % or 150 % on Windows).
9. Performance: `SHELLWING_BENCH=flood shellwing user@host` prints frames per second
   (expect the display's refresh rate); `SHELLWING_BENCH=latency` prints echo latency.
