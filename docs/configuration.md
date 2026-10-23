# Configuration

shellwing reads TOML files from its config directory:

| OS | Directory |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/shellwing` (usually `~/.config/shellwing`) |
| macOS | `~/Library/Application Support/shellwing` |
| Windows | `%APPDATA%\shellwing` |

`$SHELLWING_CONFIG_DIR` points it somewhere else.

How the files behave:

- **Every file is optional**, and so is every key; anything missing takes its
  default.
- **Edits apply to a running window** within half a second.
- **A file with a mistake is reported** with its line number, and the window
  keeps the last good values.
- **Unknown keys are ignored**, so a file written by a newer version still loads.

The `state/` folder holds what shellwing remembers by itself, such as the window's
size, place and maximised state, and the host list's width and fold
(`state/window.json`) and the hosts opened lately
(`state/recent.json`, shown in the palette), and the tabs and panes that were open
(`state/session.json`, opened again by `restore_tabs`). It is not meant for
editing, and deleting it only forgets those things. On Wayland only the size and
the maximised state come back; the compositor chooses the position.

## config.toml

**Settings** (**Ctrl+,**, **Cmd+,** on macOS, or *Settings…* in the palette)
changes these without opening the file: dark or light, theme, font, size, line height,
cursor, scrollbar, padding, the app's own font and size, scrollback, what a
dropped connection does, session logs, and whether the last tabs come back. Each change is written here at once, keeping
your comments, and applies to the open tabs.

```toml
[general]
restore_tabs = true          # a start without a host opens the last run's tabs
language = "auto"            # "auto" (the system's) or a language, see Language; this device's own

[appearance]
mode = "system"              # "system" (as the desktop is), "dark" or "light"
theme = "neon-nebula"        # the terminal's theme when dark, see below; default "dusk"
light_theme = "dawn"         # the terminal's theme when light; default "dawn"
font = { family = "JetBrains Mono", size = 14 }
line_height = 1.3            # row height as a multiple of the font size
cursor = { style = "block" } # "block", "bar" or "underline"
scrollbar = "auto"           # "auto", "always" or "never"
padding = { x = 4, y = 4 }   # pixels between the text and the pane's edges
# The app's own text (tabs, host list, palette, panels, settings), not the
# terminal's. No family: the system's interface font. Size 10 to 20, default 14,
# as the terminal's.
ui_font = { family = "Inter", size = 14 }
theme_accent = true          # the app's accent (logo, choices, buttons) is the theme's

[terminal]
scrollback = 10000           # lines of history
on_disconnect = "reconnect"  # "reconnect", "ask" or "close"

[files]
editor = "code"              # the text editor for remote files (Settings > General); none: the system's

[ssh]                        # defaults for every host; a host can set its own
host_key = "ask"             # unknown host keys: "ask", "accept-new" or "strict"
keepalive = 30               # seconds between keepalives; 0: none

[log]                        # see Session logs
always = false               # log every session from its start
folder = "~/logs/shellwing"  # default: shellwing/logs in your data folder
format = "text"              # "text" or "raw"
timestamps = true            # each line of a text log starts with the time

[vault]                      # this device's own; does not sync
lock_after = 15              # minutes idle before the vault locks; 0: never

[lock]                       # see Locking shellwing; this device's own
after = 5                    # minutes idle before shellwing locks; 0: never (the default)
```

When a connection ends, the pane shows a bar saying why and offers to connect
again (<kbd>Enter</kbd>):

- **`reconnect`** (the default): a lost connection (the network dropped, the
  server restarted) is retried by itself after 1, 2, 4, 8, 16 and 30 seconds, then
  given up; <kbd>Esc</kbd> stops the countdown. After `exit`, or when signing in
  failed, nothing happens until <kbd>Enter</kbd>.
- **`ask`**: never retries by itself.
- **`close`**: the pane closes after `exit`; a lost connection waits for
  <kbd>Enter</kbd>.

Reconnecting keeps the scrollback and resets modes a program left on (the
alternate screen, mouse reporting).

The scrollbar floats over the right edge, so it takes no column. With `auto` it
shows while you scroll (wheel, <kbd>Shift</kbd>+<kbd>PageUp</kbd>) and while the
pointer is at the right edge; drag the thumb to jump, click the track to page.
Full-screen programs (vim, htop) have no history, so they get none.

Fonts:

- **No family, or one that is not installed:** the first installed of JetBrains
  Mono and the platform's monospace fonts.
- **`$SHELLWING_FONT`** overrides the family.
- **Zoom one window:** <kbd>Ctrl</kbd>+<kbd>=</kbd> / <kbd>Ctrl</kbd>+<kbd>-</kbd>
  change the font size of that window, and <kbd>Ctrl</kbd>+<kbd>0</kbd> goes back to
  `size` (<kbd>Cmd</kbd> on macOS). The zoom is not saved.

When shellwing changes this file itself (the theme shortcut below), it edits
only that key, so your comments and layout stay as you wrote them.

## Language

shellwing speaks the system's language where it has it, else English; *Settings ›
General › Language* (`language` under `[general]`) picks another. Each device
keeps its own: it does not sync. What the terminal shows is the server's, and
stays as it is.

| `language` | |
| --- | --- |
| `en` | English |
| `de` | Deutsch |
| `es` | Español |
| `fr` | Français |
| `pt` | Português (Brasil) |
| `vi` | Tiếng Việt |
| `ru` | Русский |
| `ja` | 日本語 |
| `ko` | 한국어 |
| `zh` | 简体中文 (simplified Chinese) |

Japanese, Korean and Chinese need a font with their characters. If they show
as boxes, pick a UI font that has them in *Settings › Appearance › App text*
(Noto Sans CJK, for one).

Translations live in `crates/i18n/locales/<language>.ftl` (Fluent), each message
under the English it translates, which is in the code. `scripts/i18n.py update`
brings them in step with the code; `scripts/i18n.py todo vi` lists what is left.

## Keyboard shortcuts

*Settings › Keyboard* lists the window's shortcuts with their keys. Click a key
and press the new one (Esc stops); **+** gives a shortcut one more key, **×**
takes one away. **Reset** puts a shortcut's own keys back, **Reset all** every
shortcut's.

- **A key another shortcut has** moves to the new one when you say so; when it
  replaced a key, the two swap.
- **A key programs in the terminal use** (Ctrl+letter, Alt+letter, F-keys:
  Ctrl+R searches the shell's history, Alt+B goes back a word) is taken only
  when you say so: from then on the window has it, and the programs never get
  it. That is why the defaults are Ctrl+Shift on Linux and Windows.
- **Refused:** a key alone or with Shift alone (it is for typing), keys the
  system keeps (Alt+Tab, Alt+F4, the Windows key, Cmd+Tab, Cmd+H), and the
  pane's own keys, which cannot be changed: copy and paste, zoom, Shift+PageUp /
  PageDown / End.

The keys are kept in `config.toml`, under `[shortcuts]`, only for the shortcuts
whose keys differ from their defaults; each device keeps its own (they do not
sync, as keys differ between systems). A shortcut takes one key or a list, and
`[]` leaves it without one:

```toml
[shortcuts]
palette = ["ctrl+shift+p", "ctrl+shift+space"]
find = "ctrl+alt+f"
broadcast = []
go_to_tab = "ctrl+digit"     # Ctrl+1…9
```

A key is its modifiers and then the key, joined by `+`: `ctrl`, `alt`
(`option`), `shift`, `cmd` (`super`, `win`: Cmd on macOS, the Windows key
elsewhere), then a letter, digit or symbol, or `left`, `right`, `up`, `down`,
`home`, `end`, `pageup`, `pagedown`, `tab`, `enter`, `escape`, `space`,
`backspace`, `delete`, `insert`, `f1`…`f24`. `ctrl+shift+]` is the same key as
`ctrl+}`. `digit` stands for 1 to 9, and only `go_to_tab` takes it.

| Name | Linux, Windows | macOS |
| --- | --- | --- |
| `palette` | Ctrl+Shift+K, Ctrl+Shift+P | Cmd+K |
| `new_tab` | Ctrl+Shift+T | Cmd+T |
| `close_pane` | Ctrl+Shift+W | Cmd+W |
| `split_right` | Ctrl+Shift+D | Cmd+D |
| `split_down` | Ctrl+Shift+E | Cmd+Shift+D |
| `pane_left`, `pane_right`, `pane_up`, `pane_down` | Alt+arrows | Cmd+Option+arrows |
| `next_tab` | Ctrl+Tab, Ctrl+PageDown | Ctrl+Tab, Ctrl+PageDown, Cmd+Shift+] |
| `previous_tab` | Ctrl+Shift+Tab, Ctrl+PageUp | Ctrl+Shift+Tab, Ctrl+PageUp, Cmd+Shift+[ |
| `go_to_tab` | Alt+1…9 | Cmd+1…9 |
| `tabs` | Ctrl+Shift+A | Cmd+Shift+A |
| `host_list` | Ctrl+Shift+B | Cmd+B |
| `files` | Ctrl+Shift+O | Cmd+O |
| `find` | Ctrl+Shift+F | Cmd+F |
| `broadcast` | Ctrl+Shift+I | Cmd+Shift+I |
| `settings` | Ctrl+, | Cmd+, |
| `next_theme`, `previous_theme` | Ctrl+Shift+] / [ | Cmd+] / [ |
| `quit` | none | Cmd+Q |

The pane keys (`pane_left` …) go to the shell when there is no pane that way.
A name or key that does not read is reported and left out.

## Themes

**Dark and light.** The app is dark or light as `mode` says, or as the
desktop is with `system` (the default), and changes when the desktop does
(*Settings › Appearance › Mode*). Each mode has its own terminal theme:
`theme` when dark, `light_theme` when light. Choosing a theme, in Settings,
the palette or with the shortcut, sets the one of the mode the app is in. A
host's or a group's own theme is kept in both.

**The app's accent** (the logo, what is chosen, the main buttons) is the
theme's `accent`, unless `theme_accent = false` (*Settings › Appearance ›
The app's accent from the theme*). The app's theme gives it, not a host's;
one too close to the app's background (contrast under 3:1) is not taken.

**Changing the theme.** In the palette (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>K</kbd>,
<kbd>Cmd</kbd>+<kbd>K</kbd> on macOS), *Change theme…* lists every theme and previews
the one highlighted; Enter keeps it. <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>]</kbd> and
<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>[</kbd> (<kbd>Cmd</kbd>+<kbd>]</kbd> and
<kbd>Cmd</kbd>+<kbd>[</kbd> on macOS) step through the themes and save the
choice to `config.toml` without opening the list.

**Built in,** 69, in groups (Settings › Appearance and *Change theme…* show
them so, the light mode's first while the app is light):

- **Defaults:** `dusk`, which matches the app's own colors, and in the light
  mode `dawn`.
- **Blue and slate:** `dusk`, `default-slate`, `tokyo-night`, `nord`, `cobalt2`, `iceberg-dark`, `kanagawa-wave`, `material-ocean`, `night-owl`.
- **Purple and pink:** `dracula`, `catppuccin`, `rose-pine-moon`, `rose-pine`, `synthwave`, `ubuntu`.
- **Green and teal:** `everforest-dark`, `homebrew`, `oceanic-next`, `poimandres`, `solarized-dark`.
- **Warm:** `ayu-mirage`, `flexoki-dark`, `gruvbox-dark`, `melange-dark`, `monokai-pro`, `zenburn`.
- **Neutral:** `github-dark`, `one-dark`, `carbonfox`, `nightfox`, `oxocarbon`, `vesper`.
- **Gradient:** `neon-nebula`, `cyberpunk`, `midnight-aurora`, `sunset-glow`, `abyssal-deep`, `cosmic-velvet`, `honey-amber`, `emerald-forest`, `graphite-mist`, `crimson-ember`, `arctic-dawn`, `matrix-rain`, `aurora-borealis`, `desert-dune`, `indigo-dream`, `ocean-glass`, `sakura-night`.
- **Gradient, light:** `mint-glass`, `morning-mist`, `peach-sorbet`.
- **Light, cool:** `dawn`, `ayu-light`, `catppuccin-latte`, `github-light`, `iceberg-light`, `light-owl`, `one-light`, `tokyo-night-day`.
- **Light, warm:** `paper`, `alabaster`, `everforest-light`, `flexoki-light`, `gruvbox-light`, `kanagawa-lotus`, `rose-pine-dawn`, `selenized-light`, `solarized-light`.

Most of the solid ones come from
[iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes), as
Ghostty's do, under the MIT license (each file says so;
`crates/config/themes/LICENSE-iTerm2-Color-Schemes` has the notice). A theme
file may set `group` to one of the ids above (`blue`, `light-warm`…); one that
does not shows under *Your themes*.

**Your own themes.** They go in `themes/<id>.toml`. A file with a built-in's id
replaces that built-in. The format, with every key:

```toml
name = "My Theme"
group = "purple"                   # where it is listed; default: Your themes
accent = "#f472b6"                 # UI accents; default: palette blue
background = "#0b0e17"             # what programs see as the background
# Optional: painted behind the text instead of `background`. Angle in degrees as
# in CSS (135 = toward the bottom right); stops without a % are spread evenly.
gradient = { angle = 135, stops = ["#0b3a4f 0%", "#761a60 50%", "#0b0e17 100%"] }
foreground = "#f1f5f9"
cursor = "#34d399"                 # default: foreground
cursor_text = "#0b0e17"            # default: background
selection_background = "#f472b6"  # default: palette bright black
selection_foreground = "#ffffff"   # default: keep each character's color
palette = [                        # ANSI 0–15
  "#0b0e17", "#fc8d9d", "#34d399", "#fbbf24", "#76b2fb", "#f68bc3", "#22d3ee", "#f8fafc",
  "#9ca6b5", "#fda4af", "#6ee7b7", "#fde047", "#93c5fd", "#f9a8d4", "#67e8f9", "#ffffff",
]
```

**Programs can still set colors** (OSC 4, 10, 11, 12), as in any terminal, and a
theme change keeps their choices. If a program sets its own background, the
gradient gives way to that color.

## hosts.toml

```toml
[[group]]
name = "Production"
theme = "crimson-ember"     # for the group's hosts that set none

[[host]]
name = "api-1"
alias = "api1"              # shellwing api1
address = "10.0.1.11"
port = 2222                 # default 22
user = "deploy"             # default: from ~/.ssh/config, else your login name
group = "Production"
jump = ["bastion"]          # outermost first: hosts here or ~/.ssh/config aliases
forward = ["-L 5432:localhost:5432", "-D 1080"]   # see Port forwarding
auth = { method = "key", key = "~/.ssh/deploy_ed25519" }   # or "vault:<id>": a key the app keeps
theme = "nord"              # beats the group's and config.toml's theme
forward_agent = false       # let the host use your ssh-agent (see below)
keepalive = 15              # seconds; beats [ssh] keepalive
host_key = "accept-new"     # beats [ssh] host_key
color = "pink"              # pink, sky, green, amber, red or "#rrggbb": on its tab and pane
favorite = true             # listed first in the host list
run = "cd ~/apps"           # typed into the shell once it is ready, after every connect
log = true                  # logged from the start (false: never); beats [log] always
```

**Groups** are made with *+ Group* under the host list, *New group…* in the
palette, or *+ New group…* in a host's Group list (made when the host is saved).
Hovering a group's header shows *Edit*: its name, color and the theme of its
hosts. Renaming a group takes its hosts along; deleting one keeps its hosts,
without a group.

The host list's right edge can be dragged (200 to 480 px; a double-click goes
back to 248). The button at the right of *HOSTS* folds it to a rail of host
tiles (initials, color, status; hover for the name, click to connect), with a
button to unfold it and one to find a host in the palette. **Ctrl+Shift+B**
still hides the host list altogether.

The host list's filter matches every word typed against a host's name, alias,
address, user and tags; **Enter** opens the first host shown, **Esc** clears it.

**Agent forwarding** (`forward_agent = true`, or *Forward my agent* in the host
editor) lets the host use your local ssh-agent while you are connected: `git`
or a further `ssh` there signs in with your keys, which never leave this
machine. Whoever is root on that host can do the same while you are connected,
so turn it on only for hosts you trust. It is off by default, as in `ssh`;
`ForwardAgent yes` in `~/.ssh/config` also turns it on. Only the host connected
to gets the agent, never the jump hosts on the way, and a server that opens an
agent channel without it being asked for is refused.

**How `shellwing <name>` finds a host:**

1. A host here with that alias, then with that name.
2. An alias in `~/.ssh/config`.
3. Otherwise `<name>` is used as the host name.

A user or port typed on the command line (`root@api1:22`) wins over the file.

**Fields not used yet.** `tags` is read but has no effect yet (`id` and
`local_only` are for sync, below). The `password` and `agent` methods of `auth`
behave like the default for now: agent, then key files, then a password prompt,
as `ssh` does.

**Importing hosts.** *Import hosts from another app…* (in the palette, under
an empty host list, and *Import…* in Settings › General) reads the hosts another SSH client keeps: PuTTY and
KiTTY, MobaXterm, SecureCRT, Xshell and Tabby where they are installed, or a
file one of them exported (`.reg`, `.mxtsessions`, SecureCRT's XML or
`Sessions` folder, `.xts`), `~/.ssh/config`, or a CSV with a header row. It
shows the hosts first, by group, each with a checkbox; a host already here
(same address, port and user) is unchecked. *Import* adds the hosts checked to
this file with their groups (a folder in a folder becomes `Parent / Child`),
ports, users, key files, jump hosts and forwards, tagged `imported:<app>`; a
description becomes a note on the host. Passwords are not imported. *Undo
import from <app>* in the palette takes it all out again, until the app quits.
Termius keeps its hosts in its cloud, with no export, so they cannot be read.

**Theme order,** highest first:

1. The host's `theme`.
2. Its group's `theme`.
3. `config.toml`.

## Port forwarding

Forwards are written as `ssh`'s options, and run while the connection does:

| Spec | What it does |
| --- | --- |
| `-L [bind:]port:host:hostport` | Port `port` here reaches `host:hostport` as the server sees it. |
| `-R [bind:]port:host:hostport` | Port `port` on the server reaches `host:hostport` from here. |
| `-D [bind:]port` | A SOCKS 4/5 proxy on `port` here; each connection goes out from the server. |

Ports listen on loopback unless `bind` says otherwise; `*` means every
interface (for `-R`, the server's `GatewayPorts` decides). Port `0` takes any
free one; the one taken is shown. IPv6 addresses go in brackets: `[::1]:8080:…`.

There are four ways to forward:

- **For a host,** in hosts.toml: `forward = ["-L 8080:localhost:80"]`. Set up
  on every connection, again after a reconnect. Synced with the host.
- **In `~/.ssh/config`:** `LocalForward`, `RemoteForward` and `DynamicForward`,
  as `ssh` reads them. Every one from every matching `Host` block applies, in
  order, `Include`d files too, unless `ClearAllForwardings yes` is set. A
  hosts.toml host adds its own to those its address gets. Jump hosts forward
  nothing.
- **From the command line,** for the tab it opens:
  `shellwing -L 8080:localhost:80 -D 1080 api1`.
- **In a running tab:** *Forward a port through …* in the palette. It lasts for
  that tab, reconnects included.

What happens is shown under the connecting line: `[forwarding localhost:8080 →
localhost:80]`, or in red why a forward could not start (a port in use, a
server that refuses). The session goes on either way, as with `ssh`. A second
tab to the same host finds the first tab's ports taken and says so quietly.

Forwards under a `Match` block in `~/.ssh/config` are not applied: shellwing
does not read `Match` conditions.

## Files (SFTP)

**Ctrl+Shift+O** (**Cmd+Shift+O** on macOS), or *Files on …* in the palette,
opens the Files panel beside the focused pane. It browses the server over SFTP,
on a channel of its own, so the terminal keeps working while files move.

- Click to choose, double-click or **Enter** to open a folder or download a
  file, **Esc** to go back to the terminal. The `..` row at the top (a
  double-click), or **Backspace**, goes up to the folder above.
- **New folder**, **Rename** (**F2**) and **Delete** (**Delete**; Cmd+Backspace
  on macOS) work in the folder shown. Deleting asks first; a folder goes with
  everything in it, and a link goes as a link, never what it points at. A
  rename never writes over something already there.
- The icons at the top go home, refresh, make a folder and upload. A right
  click on a file or folder opens its menu (Open, or Edit in *your editor*
  and Open with default app; Download, Download As…, Copy path, Rename,
  Delete); a right click beside them, the folder's (New folder, Upload,
  Refresh, Copy path).
- **Download As…** asks where and under what name, for a file or a folder.
- **Download** fetches the chosen file or folder (folders whole) into your
  Downloads folder; a name already taken gets ` (1)`, ` (2)`… added. A download
  is written as `name.part` until it is complete.
- **Upload…**, or dropping files and folders on the panel, uploads into the
  folder shown.
- **Dragging a file or folder out** of the window, into a file manager or
  another app, gives it a copy (on Linux under Wayland, and on macOS; not yet
  on Windows or X11). The copy is fetched as the drag starts, into a folder of
  its own under your cache folder, and appears there only once whole, so a
  drop never takes half of it; a drop before then takes nothing. Such copies
  are removed after a day. Something already there with that name is not touched until you
  pick *Replace* (or *Merge* for a folder).
- **Edit in …** (named for the editor) opens the chosen file in the editor chosen in
  Settings > General (`[files] editor`), else the system's editor for plain
  text, whatever kind of file it is. **Open with default app** opens it in the
  system's app for its kind instead. Either way, each time you save it there,
  it is written back to the server; before that, shellwing checks that nobody else
  changed it on the server since, and asks before writing over their change.
  The copy lives in a folder of its own under your cache folder (only you can
  read it) until you press *Stop*.
- Transfers show their progress and speed and can be cancelled. Links to folders
  open as folders; when downloading a folder, links to folders inside it are
  skipped so a loop cannot run forever.

The panel keeps its folder and transfers while hidden, and opens SFTP again
after a reconnect. Its left edge can be dragged (280 to 960 px, 480 by
default; a double-click goes back to 480); the width is the same in every pane
and is remembered with the window's layout.

## Saved passwords

When a password or a key passphrase typed at a prompt works, the pane asks
whether to save it. Saved ones are tried before asking next time, also when
reconnecting, so a lost connection comes back without typing.
That includes a password a server asks for through keyboard-interactive, as
servers that sign in through PAM do (macOS among them).

They live in `vault.db` in the config directory, each one encrypted with
XChaCha20-Poly1305. The key that opens the vault is in the system keychain
(Keychain on macOS, Credential Manager on Windows, GNOME Keyring or KWallet on
Linux), so the file alone is useless. Passwords are saved per `user@host:port`,
passphrases per key fingerprint, so a key keeps its passphrase wherever it
lives. To see or forget them, open the palette and choose *Saved passwords and
passphrases…*; Enter twice forgets one. The secrets themselves are never shown.
Design and threats: `docs/design/vault.md`.

**Without a system keychain** (a window manager without GNOME Keyring or
KWallet, an SSH or remote desktop session, WSL, a container), a **passphrase**
keeps the vault's key instead. The first start asks for one (twice, at least
10 characters) and shows the vault's recovery key; each start then asks for it.
Esc leaves the vault locked, and SSH works as before without saved secrets;
*Unlock the vault…* in the palette asks again.

- The passphrase is stretched with Argon2id (about half a second and 64 MiB),
  so a copied `vault.db` is slow to guess at.
- **The recovery key always opens the vault**, synced or not: type it where the
  passphrase is asked for. It is also what opens a vault whose keychain lost
  its key (a reinstalled keyring), and it puts the key back there.
- *Settings › Vault* says where the key is kept, and sets, changes or removes
  the passphrase (removing it only while the keychain holds the key), keeps the
  key in the keychain once there is one, and shows the recovery key.

**With a system keychain**, a passphrase can still be asked for:
*Settings › Vault › Ask for it at every start* takes the key out of the
keychain, so only the passphrase (or the recovery key) opens the vault.
*Keep it in the system keychain* puts it back. While the keychain does not
hold the key:

- *Lock now* (or *Lock the vault* in the palette) closes the vault until the
  passphrase is typed again. Open connections stay; new ones ask for their
  passwords.
- *Lock when idle* (`lock_after` under `[vault]`, in minutes) locks it after
  that long without a key pressed or a click in shellwing, and asks for the
  passphrase, waiting for your return. Each device has its own.

**Windows Hello and Touch ID** open the locked vault too, once turned on in
*Settings › Vault › Unlock with Windows Hello* (or *Touch ID*), which needs a
passphrase set. When the vault locks (at start, when idle, *Lock now*), the
system asks, and the passphrase can be typed meanwhile; cancelled, the
passphrase is asked for as before. The passphrase and the recovery key keep
working.

- **Windows Hello** (a fingerprint, the face or the device's PIN): the key is
  kept by the device's security chip (TPM) and used only after Hello.
- **Touch ID** (macOS): the key is a Keychain item that opens only to Touch ID
  with the fingers enrolled now; adding one stops it until it is turned on
  again. Apple lets only signed apps keep such items: an unsigned build of
  shellwing says so.
- Linux has no such thing for keys: the passphrase it is.

## Locking shellwing

For a screen left unlocked: with *Settings › Vault › Lock shellwing when away*
(`after` under `[lock]`, in minutes), shellwing's windows hide when it starts,
and after that long without a key pressed or a click in it, until the system
says it is you:

- **macOS:** Touch ID or your Mac's password, in the system's dialog. No
  signed build needed.
- **Windows:** Windows Hello (a fingerprint, the face or the PIN), or your
  Windows password where Hello is not set up, in the system's dialog.
- **Linux:** your login password, typed on the lock and checked by PAM's
  `login` service, as screen lockers check it. Where PAM asks for a
  fingerprint first, the lock says so. A wrong password counts toward the
  system's own limit, where it has one (`pam_faillock`).

*Lock shellwing* in the palette locks it at once. While it is locked, notes
and the snippet library hide too, keys and shortcuts reach only the lock, and
*Quit shellwing* on it quits (asking first if tabs are connected).

It is a light guard, not encryption: connections, port forwards and logs go
on underneath, and anyone using your account can still read your files or run
`ssh`. Lock the screen when you leave; the vault has its own lock for the
secrets it keeps. Each device has its own setting: it does not sync.

## Local terminal

*This computer › Local terminal*, at the top of the host list (and of the
palette), opens a shell on this computer in a tab, beside the SSH ones;
`shellwing --local` opens one from the command line.

- **Splitting**, finding, snippets and typing in every pane work as with SSH.
  The Files panel and port forwarding do not: they are SSH's.
- **The shell** is yours: `$SHELL` (a login shell on macOS, as Terminal starts
  it), and on Windows PowerShell 7, else Windows PowerShell, else `cmd`. It
  starts in your home folder with `TERM=xterm-256color`, `COLORTERM=truecolor`
  and `TERM_PROGRAM=shellwing`.
- **Another shell:** set `local_shell` under `[terminal]` (*Settings ›
  Terminal › Local shell*), with its arguments:

  ```toml
  [terminal]
  local_shell = "/usr/bin/fish -l"
  ```

- **When the shell ends** (`exit`), the pane says so and Enter starts it again,
  or the pane closes when `on_disconnect = "close"`.

## Session logs

A log is a file that gets what a pane shows, as it comes. Each log is a file of
its own, named for the host and the time it started
(`api-1_2026-10-19_09-12-03.log`), and a text log starts with a line saying
which host it is (`user@address:port`) and when it started.

- **Logging one pane:** *Start Logging* in the pane's menu (right click), or
  *Log … to a file* in the palette. The log starts there, not with what is
  already on screen; *Stop Logging* ends it. While a pane is logged, a red
  *logging* badge shows at its top right; clicking it opens the file.
- **Logging every session** from its start: `always = true` under `[log]`
  (*Settings › Terminal › Log every session*). A host's own `log` wins:
  `log = false` keeps one out (a host whose output you would rather not keep),
  `log = true` logs one when the rest are not. *Session log* in the host
  editor sets it.
- **The format:**
  - `text` (the default): lines of plain text, as each was left. A progress bar
    logs its last state, a command edited at the prompt logs as it was entered,
    and colors go. What full-screen programs (vim, htop, less) draw is left
    out: it is not lines of output. With `timestamps`, each line starts with
    `[2026-10-19 09:12:03] `.
  - `raw`: every byte as it came, escape sequences and all. `cat` or `less -R`
    plays it back in color.
- **The folder:** `folder` (`~/` allowed), or *Change…* in Settings; by
  default `shellwing/logs` in your data folder (`~/.local/share` on Linux,
  `~/Library/Application Support` on macOS, `%LOCALAPPDATA%` on Windows).
  Logs hold whatever the sessions showed, so on Linux and macOS only you can
  read them (the folder is `0700`, each file `0600`).
- **A log that cannot be written** (the disk is full, the folder went away)
  stops, and the pane says why.

**Saving what a pane shows** at any time, history included, logged or not:
*Save Output as Text…* or *Save Output as HTML…* in the pane's menu, or *Save
the output of … as text / as HTML* in the palette. The HTML page keeps the
colors, on the pane's background.

## Known hosts

shellwing trusts host keys the way ssh does, through the same
`~/.ssh/known_hosts` (`$SHELLWING_KNOWN_HOSTS` replaces it).

**A host whose key changed is refused.** Its pane says so in red, with the key
known_hosts has (and its line) next to the one the server shows now. If the
server was reinstalled or its key was replaced, *Trust the new key and
connect* takes the old lines out and writes the new key, as `ssh-keygen -R`
followed by accepting the key would, then connects. If you do not know why the
key changed, do not trust it: someone may be between you and the server. A
key known_hosts marks `@revoked` is always refused.

**Settings › Known hosts** lists every key, with the host's names, its kind, its
fingerprint and its line. Find a host by name. Hashed lines show their name
only once you type it in full. *Remove* takes a line out, so the next
connection to that host asks about its key again. Every change keeps the file
as it was in `known_hosts.old`, and comments and other lines stay as they are.

## Certificates

shellwing takes OpenSSH certificates both ways, as `ssh` does, for setups where
a certificate authority (Teleport, Vault SSH, smallstep, `ssh-keygen -s`) signs
keys instead of every key being listed on every server.

**Hosts.** A line such as

```
@cert-authority *.example.com,[*.example.com]:2222 ssh-ed25519 AAAA… ca@example.com
```

in known_hosts makes shellwing ask those hosts for a host certificate and trust
one that:

- the authority signed;
- is a host certificate, still valid;
- names the host among its principals (or names none);
- is not `@revoked`, nor is its authority.

Nothing is learned into known_hosts. A certificate that fails these checks is
still accepted if known_hosts knows the key inside it; otherwise the pane says
why it was refused. Names in known_hosts may be patterns (`*`, `?`, and
`!name` to leave one out), as in OpenSSH.

**Signing in.** A certificate for a key is used before the key alone. It is
found:

- beside the key file, as `<key>-cert.pub` (`~/.ssh/id_ed25519-cert.pub`);
- or named by `CertificateFile` in `~/.ssh/config`;
- or held by ssh-agent with its key (`ssh-add` adds `id_ed25519-cert.pub` with
  `id_ed25519`).

## Snippets

Snippets are commands you run often. They are kept in the vault (`vault.db`,
encrypted, never in a file of their own) and sync with it. In *Settings ›
Snippets*, **New snippet…** or a snippet's edit button opens it in a window of
its own, as a note: its name and category above, its command in a code editor,
and beside it the hosts it is offered for (every host, a group or one host),
the environments it needs, its `{{name}}` values and whether Enter is pressed
after typing. With many snippets, a category files them: Settings and the
palette list them under it (*Snippets · Deploy*).

- **The editor** colors the command as shell (commands, flags, quoted text,
  `$` expansions, operators, comments) and its `{{name}}` values, and numbers
  its lines. Tab indents with two spaces; Enter keeps the line's indent.
- **Saving** is as for notes: a moment after typing stops, once it has a name
  and a command, and when the window closes; Ctrl+S (Cmd+S) saves at once.
  While its window is open, the snippet is left out of syncs.
- **Run in {pane} / Insert into {pane}** below run it in the main window's
  focused pane (its values asked for there), or type it without Enter. A
  snippet that needs environments only runs: typed alone it could not have
  them.

A command can take several lines. It is sent to the pane as one paste, so a
shell with bracketed paste on (bash, zsh and fish are, by default) takes it
whole and runs it at the Enter; lists show it on one line, ` ⏎ ` between the
lines.

In the palette (Ctrl+Shift+K), a pane's snippets are listed under *Snippets*;
*Snippets…* in the pane's right-click menu opens the palette there. Under the
palette's field, a row of badges (Hosts, Recent, Snippets, Environments,
Actions, Settings) jumps to each part of the list; so do Tab and Shift+Tab.

- **Enter** types the snippet into the pane.
- **Alt+Enter** (Option+Enter on macOS) types it into every pane of the tab;
  while typing in every pane, only into the panes that listen.

In the command, `{{host}}`, `{{user}}` and `{{address}}` are filled in for each
pane's host. Any other `{{name}}` is asked for first, with the command shown as
it will run. A name starts with a letter or `_`: Go templates such as Docker's
and kubectl's (`--format '{{.Names}}'`) stay as they are.

### The snippet library

*Settings › Snippets › Library…* (or *Snippet library…* in the palette) opens
about 250 ready commands, by tool on the left and by task above (Diagnostics,
Logs, Maintenance, Deploy, Backup, Database, Data, Testing), with a search:

- **The machine:** the system (disk, memory, processes, kernel errors, OOM
  kills), systemd (services, timers, the journal), the network (ports,
  connections, DNS, HTTP timing, firewalls), files (largest, recent, deleted
  but open, tar, rsync), security (failed SSH logins, fail2ban, certificates),
  packages (apt, dnf) and performance (vmstat, iostat, iotop).
- **What runs on it:** Docker, Docker Compose, Docker Swarm, Kubernetes, web
  servers (nginx, Apache, Caddy), RabbitMQ, Kafka, Git checkouts.
- **Databases:** PostgreSQL, MySQL, MariaDB, Redis, MongoDB, ClickHouse,
  Elasticsearch, InfluxDB, SQLite, SQL Server, Dolt, migration tools.
- **Backups and clouds:** restic, Borg, AWS, Google Cloud, Azure; load tests. Tick those you want and
**Add to my snippets**: each becomes a snippet of your own, to edit as any
other. What was added is marked *Added* (and listed under *Added*); a command
can be added again, as another copy.

- **Before adding**, the one shown on the right can be adjusted: its options
  (the tool's flags it does not take by default), **sudo** (on for what needs
  root, before each command of it that needs it), and whether it runs on the
  host or in a Docker container (`docker exec -it {{container}} …`, the
  container asked when it runs).
- **Passwords** are never in a command: the tool reads its own variable
  (`PGPASSWORD`, `MYSQL_PWD`, `REDISCLI_AUTH`…) or `$DB_PASSWORD`, from the
  environment the snippet is given; when one environment has that variable it
  is given by default. Without one, the tool asks for it, or goes without.
- **Values** such as `{{db_user}}` and `{{database}}` are asked when it runs.
- **What makes changes** (restarts, deploys, scaling, restores, imports,
  migrations, drops, prunes, upgrades) is marked *Makes changes* and added
  typed, not run: it waits for Enter.
- **File under** sets the new snippets' category: their tool (*PostgreSQL*),
  their task (*Backup*, the name then saying the tool), or both.

The library's text is in English. Its commands are in two files:
`crates/app/src/library/extra.toml`, written by hand (the machine, what runs on
it, backups and clouds), and `crates/app/src/library/items.json`, made from
HivePaaS's command templates by `scripts/library_from_hivepaas.py` (the
databases).

## Environments

Environments are named sets of variables for the shell, such as an API's
keys: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`. They are kept in the vault,
encrypted, and sync with it. Make them in *Settings › Environments*: a name, the
hosts they are offered for, and the variables, a name and a value a row. A value
can take several lines (a PEM key, say): Enter starts another, Ctrl+Enter
(Cmd+Enter on macOS) saves. **Paste a .env file** fills the rows from `.env`
text on the clipboard (`export`, `#` comments and quotes are taken).

Once saved, values are masked: editing shows the names, and a masked value is
kept unless you change it. **Show values** shows them, after asking it is you:
Windows Hello or Touch ID where it opens the vault, else the vault's
passphrase; with neither, a yes. They are masked again a minute later.

Nothing is loaded by itself. You choose when, and what:

- The palette lists the focused pane's under *Environments*, while it is
  connected: Enter loads one into its shell (*Load Environment…* in the pane's
  right-click menu opens the palette there). The pane shows a badge for each
  one loaded. **Unload environment** in the palette or the menu, or the
  badge's ✕, unsets its variables.
- A snippet that **needs** environments runs in a subshell that has them, and
  they are gone when it ends; the pane's shell never gets them.

To load, shellwing types a command that turns the terminal's echo off, says it
is ready (with an escape sequence its terminal reads and never shows), and only
then sends the values for `read` to take: as lines of printable ASCII, with a
newline, a control character or a byte past ASCII written `\0ooo` for the
shell's `printf %b` to turn back. The command holds the names only, and starts with a space, which keeps it out of the history of
bash (with `HISTCONTROL=ignorespace`, the default on most systems) and zsh
(with `HIST_IGNORE_SPACE`). A POSIX shell (sh, bash, zsh) at its prompt is
needed; not while a full-screen program runs; fish is not supported yet.

**Limits.** A shell takes an exported variable of 128 KB at most on Linux, and
all of them with a command's arguments in about 2 MB: past that, every program
it starts fails ("Argument list too long"). So shellwing keeps well within: a
variable's name takes 128 characters, a value 32 KB (a PEM key is 3–4 KB), an
environment 100 variables and 64 KB, and the environments loaded into one
shell 256 KB together. A snippet's command takes 64 KB.

While loaded, the variables are in the shell's memory on the server: whoever
has root there, or runs as your user, can read them, until they are unloaded or
the shell ends (unloading frees them in the shell, though not every copy a
program made). Load them on servers you trust, unload them when done, and
prefer credentials that expire or can do little: short-lived tokens, or a role
with only what the work needs.

## Config files

Config files are files to place on hosts, such as `~/.kube/config`,
`~/.aws/credentials` or `~/.npmrc`. They are kept in the vault, encrypted, and
sync with it; one holds 1 MB at most, text or not. Make them in *Settings ›
Config files*: a name, a category, where the file goes on the host (from `/` or
`~/`), its permissions (`600` by default), *When placed* (whether placing it
offers *until the tab closes* or *to stay* first), the hosts it is offered for,
and its content. Text up to 64 KB is edited there; **Choose a file…** takes any file up
to 1 MB from this computer (one under your home folder goes to the same place
under the host's). *Keep a File in the Vault…* in the pane's right-click menu
takes one from the host instead.

Once saved, the content is masked: the eye shows it, after asking it is you, as
for an environment's values.

**Placing one.** The palette lists the focused pane's host's under *Config
files*, while it is connected (*Place Config File…* in the pane's right-click
menu opens it there). Enter asks where (the file's own path, to change if
need be), then how, the file's *When placed* first (Enter):

- **Until the tab closes** (the default): the pane shows a badge; closing the
  tab, the pane or the app takes the file away, as does the badge's ✕ or
  *Remove …* in the menu. The badge's **Keep** (or *Keep … on the host* in the
  menu, or placing it again) leaves it there for good instead: what it set
  aside becomes `….bak`.
- **To stay**: it is left there.

If a file is there already (and differs), it asks: replace it until the tab
closes (the one there is set aside beside it, as `….shellwing-saved`, and comes
back), replace it and keep the old one as `….bak`, or replace it.

The file is written whole: to a file beside it first, with its permissions from
the start, then renamed over the target; folders missing on the way are made
with `700`. If the connection drops, or the app is killed, before a file placed
until the tab closes is taken away, it is taken away the next time the host
connects (and what it set aside put back), unless it changed since: then it is
left as it is, and said so.

A placed file is on the server's disk: whoever has root there can read it until
it is removed. Place secrets on servers you trust, until the tab closes.

## Notes

Notes are free text kept in the vault, encrypted, and synced with it: what a
server is for, how to restart it, codes to keep. One holds 256 KB at most. Each
opens in a window of its own, from *Settings › Notes*, or from the palette
(under *Notes*; *New note…* there too). The palette lists first the notes
about the focused pane's host.

A note's window opens at the size the last one had when it closed (800 × 700
the first time; never less than 600 × 500), kept in `state/note-window.json`.

In its window, a note has a title, a category, a format and the hosts it is
about (*For a host ▾*):

- **Plain text** is edited as it is, in the terminal's font.
- **Markdown** shows as it is typed: headings, emphasis, `code`, code blocks,
  lists and task lists, quotes, GitHub alerts, rules, tables; the marks
  (`**`, `#`) show on the caret's line only. Enter carries a list on (an empty
  item ends it), Tab and Shift+Tab indent; a table is edited in its cells; a
  task's box ticks on a click. The right-click menu cuts, copies (also as
  Markdown), pastes, turns a line into another kind of block, and edits a
  table. *Preview* shows it read-only, laid out as when it is edited: it
  selects and copies, a task's box ticks, nothing else changes it. A code
  block has *Copy* and *Insert* (into the main window's pane) at its top.

**Images.** A Markdown note shows images: paste one (a screenshot on the
clipboard, or image files copied), drop image files on its window, or choose
*Insert Image…* in the editor's menu. Each goes in as a line of its own,
`![](attachment:…)`, and shows as the picture; its grip resizes it. Images
are kept in the vault, encrypted, and sync with it. Before they are kept they
are scaled to 2560 px on their long edge and written as PNG (screenshots) or
JPEG (photos), which drops their metadata, such as where a photo was taken;
one holds 2 MB at most, and a note's 20 MB together. An image no note has
shown for 30 days is removed. While a synced note's image has not come yet,
its line says so, and the picture shows once it does. Right-click an image
for *Copy Image* and *Save Image As…* (in Preview too), or *Delete image*.
Dropped files go where they are dropped, after the line under the pointer.
Markdown pasted from elsewhere with images as `data:` URIs keeps them, as
attachments. *Export…*, beside *Delete*, writes the note as `note.md` with
its images in `images/`, in a folder you choose; it asks first, as the files
are not encrypted. Images linked from the web (`![](https://…)`) are never
fetched by themselves: they show where they are, a click opens them in the
browser, and *Download Images* in the note's toolbar keeps a copy of each, as
an attachment, after saying which servers it contacts. Images on your own
network (this computer, a LAN) are not downloaded. Design:
[docs/design/note-images.md](design/note-images.md).

The editor is [zorite-editor](https://crates.io/crates/zorite-editor), taken
into shellwing (`crates/note-editor`, see its `UPSTREAM.md`): a note loads as
it is, and Obsidian's constructs (`[[links]]`, `#tags`, `key:: value`) and
math (`$`, `$$`) are plain text, as commands and paths are. The read-only view
is the same editor, read-only.

Ctrl+Z (Cmd+Z) undoes, Ctrl+Shift+Z (Cmd+Shift+Z) redoes. A long note scrolls,
with a bar to drag. Text wraps to the window; a table wider than it scrolls
sideways, by its bar, a sideways swipe, or Shift and the wheel.

A new note starts with a short guide, in the app's language: what Markdown
does there, a code block, and how it goes into the shell. Write over it;
closed untouched, a new note is not kept.

**Selected text** gets a bar over it: *Copy*, and *Insert into …*, which types
it into the main window's focused pane (connected), without Enter, and brings
that window forward to read it and run it. Several lines go as one paste. In a
Markdown note the bar also formats: bold, italic, strikethrough, code (a fenced
block for several lines), link, heading, quote, list, task; applied again, each
comes off. Ctrl+B, Ctrl+I, Ctrl+E and Ctrl+K (Cmd on macOS) do bold, italic,
code and link.

It is saved on this device three seconds after it last changed (nothing is
written when it is as last saved), when its window closes, before the vault
locks, and when the app quits; Ctrl+S (Cmd+S on macOS) saves at once. *Delete*
removes it, after asking.

**Syncing.** While a note is open in its window, syncs leave it alone (both
ways): it goes out when the window closes, at once if it changed, and when the
app quits (the app syncs before it goes, ten seconds at most). The rest of the
vault syncs as before: when it changes, and every minute.

A note's text changed on two devices before either saw the other's change
keeps both: the later edit stays the note, and the other becomes a note beside
it, titled *Disk usage (from laptop)* after the device that wrote it; the
window says so. Delete it once its text is taken in.

While the vault is locked, a note's window holds no text: it comes back once
the vault is open. A **sensitive** note's text shows only after asking it is
you, as for an environment's values. Closing the main window closes the notes'
windows too.

## Typing in every pane

**Ctrl+Shift+I** (**Cmd+Shift+I**) types what you type in one pane into every
pane of the tab, as tmux's `synchronize-panes` does. Each pane gets the key
itself, so the program running there reads it with its own modes.

While it is on:

- the tab shows an amber dot;
- a strip above the panes says how many panes listen;
- listening panes have an amber edge;
- a pane's badge leaves it out (dimmed) or brings it back.

Ctrl+Shift+I again, *Stop* on the strip, or leaving the tab with one pane
stops it. Passwords and passphrases that shellwing itself asks for are never
copied to other panes.

## SSH keys

*Settings › Keys* lists the keys there are, in three groups:

- **In the vault.** These are keys generated or imported here. They are kept in
  `vault.db` without a passphrase (the vault is encrypted already), and they
  sync with the rest of the vault.
- **In ~/.ssh.** These are key files, shared with ssh and git. Each one shows
  whether it has a passphrase and whether the vault has saved it. *Copy to
  vault* keeps a copy of the key in the vault; the file stays where it is.
- **In ssh-agent.** These are the agent's keys.

*Generate key…* makes an Ed25519 key (ECDSA P-256 and RSA 4096 too) and keeps
it in the vault. *Export to file…* writes a kept key out for other tools, with
its `.pub` beside it and an optional passphrase.

Each key's ⋯ menu has more:

- **Add to ssh-agent.** Available for kept keys and key files, so that ssh, git
  and a forwarded agent can sign with the key until the agent stops. *Remove
  from ssh-agent* takes it out again.
- **Save its passphrase…** For a key file with a passphrase. Once the vault
  keeps the passphrase, signing in with that key does not ask, here or on your
  other devices. *Forget its saved passphrase* undoes it.

In the host editor, the *Key* list can also *Generate a key for* the host. The
new key is kept in the vault and chosen for the host. *Save and install on …*
then saves the host, connects once another way (with a password, for example)
and adds the key to the host.

*Install on…* adds a public key to a host's `~/.ssh/authorized_keys`, as
`ssh-copy-id` does. It runs over the connection already open, or opens one
first. Afterwards, a host in `hosts.toml` can be set to sign in with that key
from then on. That writes `key = "vault:<id>"` in its `auth`, and the host
editor offers the same choice.

Hosts set to sign in automatically try the agent, then `~/.ssh`, then every key
in the vault, then a password. A host that names a key offers only that key.

## Syncing passwords between devices

```toml
[sync]
folder = "~/Library/Mobile Documents/com~apple~CloudDocs/shellwing"   # any synced folder
settings = true             # sync the settings that are not this device's own
```

*Settings › Sync* is where sync is set up and followed: how it stands (when it
last synced, or why not, with Sync now or Try again), what travels, the place
and its recovery key, and the options; shellwing cloud beside them. The cloud
in the header says the same in brief, in its color (green synced, orange
failed, a light orange not set up), and opens Settings › Sync.

Choose *Sync passwords with a folder…* in the palette, or set the folder here.
Any folder another tool keeps in sync works: iCloud Drive, Syncthing, a Dropbox
or OneDrive folder, a network share. shellwing writes `shellwing/format.json` and
one encrypted file per saved secret in `shellwing/records/`; nothing in them is
readable without the key.

**A server instead of a folder:** *Settings › Sync* (or *Sync passwords with a
server…* in the palette) takes a WebDAV server or S3-compatible storage, one
place at a time:

```toml
[sync]
# WebDAV: Nextcloud, ownCloud, Synology, Koofr, Fastmail. The folder's address.
webdav = { url = "https://cloud.example.com/remote.php/dav/files/me/", user = "me" }
# Or S3-compatible: AWS S3, Cloudflare R2, Backblaze B2, MinIO, Wasabi.
# region: if the service asks for one (R2 takes "auto", the default).
# prefix: a folder in the bucket; none: the top.
s3 = { endpoint = "https://<account>.r2.cloudflarestorage.com", bucket = "vault", prefix = "me", access_key = "…" }
```

- The password (WebDAV) or secret key (S3) is not in the file: it is kept in
  the vault, sealed, as this device's own, and never synced. Each device
  signs in with its own; *Stop syncing* forgets it. An app password, or a key
  limited to the bucket, is the safer choice.
- The files are the same as in a folder, under `shellwing/` at the address
  (or under the prefix). The server only ever sees them encrypted.
- Each sync lists the records and fetches only those that changed since
  (by their ETag), so a minute's sync with nothing new is a few small
  requests.
- A record another device wrote a moment ago is not written over: the
  server refuses the write (`If-Match`), and shellwing merges theirs in
  and writes again.
- Joining from another device works as with a folder: pairing, the recovery
  key, or the sync passphrase.

**A Git repository:** *Settings › Sync › Git* takes a private repository on
GitHub, GitLab, Gitea or a server of your own:

```toml
[sync]
git = { url = "git@github.com:me/vault.git" }   # or https://…; branch = "…" if not the default
```

- It goes through **this system's git** (2.31 or newer), which signs in as it
  does for you: Git Credential Manager, the keychain, `gh auth`, your SSH keys
  and agent. Where git cannot sign in by itself, a token (with access to that
  repository only) can be set; it is kept in the vault like a password, and
  given to git in its environment, not on its command line. Gitea needs the
  token's user too.
- **No git yet?** Connect says so and offers to install it: Apple's Command
  Line Tools on macOS (in Apple's own dialog), winget on Windows (which asks to
  allow it), or the command for your distribution on Linux, to run in a
  terminal. A git that is too old or does not work is said too.
- shellwing keeps a clone of its own in the cache folder and never merges with
  Git: each sync fetches, sets the clone to what the repository has, merges
  field by field as with a folder, then commits (`Sync from <id>`, as
  `shellwing`) and pushes. A push refused because another device pushed first
  is done again over theirs. The commits name each device by a random id, not
  its name, and carry the time in UTC, not your time zone.
- **A repository anyone can read** is asked about at Connect: the vault is
  encrypted there, but anyone can see how many hosts it holds and when your
  devices sync. *Use another repository* connects nothing; *Use it anyway*
  goes on.
- git never waits for a password where nobody can type it: SSH runs with
  `BatchMode` (unless you set how SSH runs for git), so a server whose key SSH
  does not know yet is said; connect to it once with ssh to accept it. Your
  own hooks and commit signing are left out of these commits.
- Each change is a small commit. The history grows by a few KB a change, which
  is fine for years.

- **The first device** creates a **recovery key** (13 groups of 4 characters) and
  shows it: write it down. *Show the recovery key* shows it again.
- **Another device** choosing the same folder joins the vault one of two ways;
  either way what it had saved is kept and merged:
  - **Approve it on a device that syncs** (pairing). The new device asks; on a
    device that syncs the folder (with shellwing open), *Let … into this
    vault?* comes up. Both show a six-digit code: approve only if they are the
    same. The keys travel encrypted, through the folder; the new device can
    show the recovery key afterwards too. A request left 10 minutes is dropped.
  - **Type the recovery key.**
  - **Type the sync passphrase**, if a device that syncs set one
    (*Settings › Sync › Let a passphrase join new devices*). It is kept in
    the folder, sealed by itself, so anyone who can read the folder can guess
    at it: it must be hard to guess (several unrelated words; shellwing
    refuses easy ones). Turning the option off removes it.
- Syncing runs every minute, right after a secret is saved or forgotten, and on
  *Sync now*. Edits merge field by field, so two devices offline never lose
  each other's changes; a forgotten secret is forgotten everywhere.
- *Stop syncing* leaves the folder and this device's secrets as they are.

Hosts and groups in `hosts.toml` sync the same way, encrypted like the secrets:

- Each host and group gets an `id` the first time (added to the file, first in
  its table); keep it when editing.
- Edits merge field by field: change a port on one device and a user on another,
  and both changes arrive. A host added elsewhere is appended to the file; one
  deleted elsewhere is removed. Comments and the lines nobody changed stay as
  written.
- `local_only = true` keeps a host on this device.
- A missing `hosts.toml` is written again from the synced hosts, never taken as
  "delete them all".

Settings in `config.toml` sync too, unless `settings = false` under `[sync]`
(*Settings › Sync › Sync settings too*):

- **What syncs:** `[general] restore_tabs`; all of `[appearance]` (dark or
  light, both themes, fonts and sizes, line height, cursor, scrollbar,
  padding); `[terminal] scrollback` and `on_disconnect`; `[ssh] host_key` and
  `keepalive`; `[log] always`, `format` and `timestamps`.
- **What each device keeps:** `[sync] folder`, `[log] folder`,
  `[terminal] local_shell` and `[files] editor`, which name things of that
  computer.
- **Merging** is per setting, as for hosts; the file keeps its comments and
  layout. A device whose settings sync for the first time takes the others'
  where they set one, and keeps its own where they set none.
- A value this version does not know (written by a newer one) is left out
  rather than breaking the file.

