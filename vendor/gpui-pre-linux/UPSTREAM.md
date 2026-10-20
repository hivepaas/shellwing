# gpui-pre-linux, taken in

GPUI's Linux platform (X11 and Wayland), as published on crates.io.

- **Upstream:** `gpui-pre-linux` 0.3.8 on crates.io, "Zed's `gpui_linux` crate
  (gpui-pre snapshot of zed@279fe07)", <https://github.com/zed-industries/zed>
  (`crates/gpui_linux`). License: Apache-2.0 (`LICENSE-APACHE`).
- **As published:** commit `d251fc3` ("Take in gpui-pre-linux 0.3.8 as published,
  unchanged") holds the crate exactly as crates.io serves it (`.cargo-ok` left
  out). Diff against it to see every change: `git diff d251fc3 -- vendor/gpui-pre-linux`.
- **How it is used:** the workspace's `Cargo.toml` puts it in place of the
  crates.io one (`[patch.crates-io]`) and keeps it out of the workspace
  (`exclude = ["vendor"]`), so our lints and `cargo fmt` leave it as it is.
- **Finding the changes:** each is marked `// shellwing:`;
  `grep -rn "shellwing:" vendor/gpui-pre-linux/src`.

## Changes

1. **Wayland: a window brought forward from another of the app's
   (`src/linux/wayland/window.rs`, `WaylandWindow::activate`; and
   `src/linux/wayland/client.rs`, `WaylandClientStatePtr::keyboard_focused_surface`,
   new).** The xdg-activation token was asked from the surface of the window to
   activate, which does not have the keyboard and did not get the serial, so
   GNOME (Mutter) and KDE refused it: the main window stayed behind when a note
   window inserted text into a pane, or a snippet window ran one. The token is
   now asked from the surface that has the keyboard (the window the user is
   in), falling back to the window's own; it still activates the window it was
   asked for.

## Taking in a newer upstream

1. Check whether upstream fixed what a change here works around (the comment
   in `activate` above the token says it expects to be refused); drop the
   change if so. As of 0.3.9 it is the same.
2. Replace the folder with the new crate as published (from
   `~/.cargo/registry/src/*/gpui-pre-linux-<version>`, without `.cargo-ok`),
   commit that alone, and note its commit above.
3. Make each change again where its `shellwing:` comment was, and update
   this file: the version, the commit, and each change.
4. Keep its version the one `gpui-pre` (the workspace's `gpui`) wants:
   `[patch.crates-io]` only replaces the same version.
