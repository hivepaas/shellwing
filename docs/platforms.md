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

## Toolchain (to build locally)

All platforms need Rust (via [rustup](https://rustup.rs)) and **Zig 0.16**
from [ziglang.org/download](https://ziglang.org/download/) on `PATH`;
libghostty-vt is compiled from source with it.

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

## The core on iOS and Android

There is no mobile app yet, but `shellwing-core` (term, ssh, config, vault) builds
for the phones, for a front end to link. The vault's key store comes from the app
there (`VaultSlot::open_with`): the keyring crate picks none on iOS or Android.

- iOS: Xcode with the iOS SDK. If `xcode-select -p` shows the Command Line Tools,
  set `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`, then
  `rustup target add aarch64-apple-ios-sim` and
  `cargo build -p shellwing-core --target aarch64-apple-ios-sim`.
- Android: the NDK (`brew install --cask android-ndk` on macOS), with
  `ANDROID_NDK_HOME` pointing at it, and its clang for the C parts:

  ```sh
  bin="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin"  # linux-x86_64 on Linux
  export CC_aarch64_linux_android="$bin/aarch64-linux-android24-clang"
  export AR_aarch64_linux_android="$bin/llvm-ar"
  rustup target add aarch64-linux-android
  cargo build -p shellwing-core --target aarch64-linux-android
  ```

CI builds the Android core on every pull request; iOS when started by hand
(Actions > CI > Run workflow, `ios` or `all`).

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
