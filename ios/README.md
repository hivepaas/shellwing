# shellwing for iOS

A SwiftUI app over the same core as the desktop: `crates/mobile` wraps
`shellwing-core` (SSH, the terminal's libghostty-vt, hosts.toml) for Swift
with [UniFFI](https://mozilla.github.io/uniffi-rs/). The app draws the
terminal's frames and sends it what is typed; connecting, prompts (passwords,
host keys) and the terminal itself are the desktop's own code.

## Building

Needs Xcode, Zig 0.17 on `PATH`, and XcodeGen (`brew install xcodegen`).

```sh
make ios        # the Rust library for iPhones and the Simulator, its Swift, the Xcode project
open ios/Shellwing.xcodeproj
```

Then Run in Xcode, on a Simulator (Apple silicon) or an iPhone; signing uses
the team in `ios/project.yml`. `make ios-sim` builds for the Simulator from
the command line. After changing `crates/mobile`, run `make ios` again.

What is generated, and not committed: `ios/Generated/` (the XCFramework and
`shellwing_mobile.swift`), `ios/Shellwing.xcodeproj` and its `Info.plist`.
Edit `ios/project.yml` for the project's settings.

Where GitHub is slow to clone from, `GHOSTTY_SOURCE_DIR=<a ghostty checkout>`
reuses one, such as the one a desktop build left under
`target/release/build/libghostty-vt-sys-*/out/ghostty-src`.

## Trying it

`-open <host>` as a launch argument (debug builds) opens a session to that
host at once. The app keeps `hosts.toml` in its own Application Support
folder; `xcrun simctl get_app_container booted com.hivepaas.shellwing data`
finds it in the Simulator.

## What is there

| | |
| --- | --- |
| `Shellwing/AppModel.swift` | The core, the hosts, the open sessions; each session's frames and status |
| `Shellwing/TerminalView.swift` | The terminal: draws a frame's runs, takes the keyboard, scrolls the history |
| `Shellwing/TerminalScreen.swift` | A session full screen, with the keys a phone lacks (Esc, Tab, Ctrl, Alt, arrows) |
| `Shellwing/HostsView.swift`, `HostEditorView.swift` | The host list by group, and a host's fields |
| `Shellwing/SessionsView.swift` | Every open session, as the desktop's tab overview |
| `Shellwing/AppLock.swift` | Lock shellwing when away: Face ID or the passcode |

Not yet: the vault (saved passwords and keys), sync, snippets and config
files, SFTP.
