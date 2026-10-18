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
size, place and maximised state (`state/window.json`) and the hosts opened lately
(`state/recent.json`, shown in the palette), and the tabs and panes that were open
(`state/session.json`, opened again by `restore_tabs`). It is not meant for
editing, and deleting it only forgets those things. On Wayland only the size and
the maximised state come back; the compositor chooses the position.

## config.toml

**Settings** (**Ctrl+,**, **Cmd+,** on macOS, or *Settings…* in the palette)
changes these without opening the file: theme, font, size, line height,
cursor, scrollbar, padding, the app's own font and size, scrollback, what a
dropped connection does, and whether the last tabs come back. Each change is written here at once, keeping
your comments, and applies to the open tabs.

```toml
[general]
restore_tabs = true          # a start without a host opens the last run's tabs

[appearance]
theme = "neon-nebula"        # a theme id or name, see below; default "default-slate"
font = { family = "JetBrains Mono", size = 14 }
line_height = 1.3            # row height as a multiple of the font size
cursor = { style = "block" } # "block", "bar" or "underline"
scrollbar = "auto"           # "auto", "always" or "never"
padding = { x = 4, y = 4 }   # pixels between the text and the pane's edges
# The app's own text (tabs, host list, palette, panels, settings), not the
# terminal's. No family: the system's interface font. Size 10 to 20, default 14,
# as the terminal's.
ui_font = { family = "Inter", size = 14 }

[terminal]
scrollback = 10000           # lines of history
on_disconnect = "reconnect"  # "reconnect", "ask" or "close"

[ssh]                        # defaults for every host; a host can set its own
host_key = "ask"             # unknown host keys: "ask", "accept-new" or "strict"
keepalive = 30               # seconds between keepalives; 0: none
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

## Themes

**Changing the theme.** In the palette (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>K</kbd>,
<kbd>Cmd</kbd>+<kbd>K</kbd> on macOS), *Change theme…* lists every theme and previews
the one highlighted; Enter keeps it. <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>]</kbd> and
<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>[</kbd> (<kbd>Cmd</kbd>+<kbd>]</kbd> and
<kbd>Cmd</kbd>+<kbd>[</kbd> on macOS) step through the themes and save the
choice to `config.toml` without opening the list.

**Built in.**

- **Default:** `default-slate`.
- **Gradients:** `neon-nebula`, `cyberpunk`, `midnight-aurora`, `sunset-glow`,
  `abyssal-deep`, `cosmic-velvet`, `honey-amber`, `emerald-forest`,
  `graphite-mist`, `crimson-ember`, `arctic-dawn`, `matrix-rain`.
- **Solid:** `dracula`, `tokyo-night`, `one-dark`, `nord`, `catppuccin`,
  `github-dark`.

**Your own themes.** They go in `themes/<id>.toml`. A file with a built-in's id
replaces that built-in. The format, with every key:

```toml
name = "My Theme"
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
auth = { method = "key", key = "~/.ssh/deploy_ed25519" }
theme = "nord"              # beats the group's and config.toml's theme
forward_agent = false       # let the host use your ssh-agent (see below)
keepalive = 15              # seconds; beats [ssh] keepalive
host_key = "accept-new"     # beats [ssh] host_key
color = "pink"              # pink, sky, green, amber, red or "#rrggbb": on its tab and pane
favorite = true             # listed first in the host list
run = "cd ~/apps"           # typed into the shell once it is ready, after every connect
```

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

There are three ways to forward:

- **For a host,** in hosts.toml: `forward = ["-L 8080:localhost:80"]`. Set up
  on every connection, again after a reconnect. Synced with the host.
- **From the command line,** for the tab it opens:
  `shellwing -L 8080:localhost:80 -D 1080 api1`.
- **In a running tab:** *Forward a port through …* in the palette. It lasts for
  that tab, reconnects included.

What happens is shown under the connecting line: `[forwarding localhost:8080 →
localhost:80]`, or in red why a forward could not start (a port in use, a
server that refuses). The session goes on either way, as with `ssh`. A second
tab to the same host finds the first tab's ports taken and says so quietly.

`LocalForward`, `RemoteForward` and `DynamicForward` in `~/.ssh/config` are not
applied yet; put them in hosts.toml instead.

## Files (SFTP)

**Ctrl+Shift+O** (**Cmd+Shift+O** on macOS), or *Files on …* in the palette,
opens the Files panel beside the focused pane. It browses the server over SFTP,
on a channel of its own, so the terminal keeps working while files move.

- Click to choose, double-click or **Enter** to open a folder or download a
  file, **Backspace** to go up, **Esc** to go back to the terminal.
- **New folder**, **Rename** (**F2**) and **Delete** (**Delete**; Cmd+Backspace
  on macOS) work in the folder shown. Deleting asks first; a folder goes with
  everything in it, and a link goes as a link, never what it points at. A
  rename never writes over something already there.
- **Download** fetches the chosen file or folder (folders whole) into your
  Downloads folder; a name already taken gets ` (1)`, ` (2)`… added. A download
  is written as `name.part` until it is complete.
- **Upload…**, or dropping files and folders on the panel, uploads into the
  folder shown. Something already there with that name is not touched until you
  pick *Replace* (or *Merge* for a folder).
- Transfers show their progress and speed and can be cancelled. Links to folders
  open as folders; when downloading a folder, links to folders inside it are
  skipped so a loop cannot run forever.

The panel keeps its folder and transfers while hidden, and opens SFTP again
after a reconnect.

## Saved passwords

When a password or a key passphrase typed at a prompt works, the pane asks
whether to save it. Saved ones are tried before asking next time, also when
reconnecting, so a lost connection comes back without typing.

They live in `vault.db` in the config directory, each one encrypted with
XChaCha20-Poly1305. The key that opens the vault is in the system keychain
(Keychain on macOS, Credential Manager on Windows, GNOME Keyring or KWallet on
Linux), so the file alone is useless. Passwords are saved per `user@host:port`,
passphrases per key fingerprint, so a key keeps its passphrase wherever it
lives. To see or forget them, open the palette and choose *Saved passwords and
passphrases…*; Enter twice forgets one. The secrets themselves are never shown.
Design and threats: `docs/design/vault.md`.

## Syncing passwords between devices

```toml
[sync]
folder = "~/Library/Mobile Documents/com~apple~CloudDocs/shellwing"   # any synced folder
```

Choose *Sync passwords with a folder…* in the palette, or set the folder here.
Any folder another tool keeps in sync works: iCloud Drive, Syncthing, a Dropbox
or OneDrive folder, a network share. shellwing writes `shellwing/format.json` and
one encrypted file per saved secret in `shellwing/records/`; nothing in them is
readable without the key.

- **The first device** creates a **recovery key** (13 groups of 4 characters) and
  shows it: write it down. *Show the recovery key* shows it again.
- **Another device** choosing the same folder is asked for that key, then
  shares the vault; what it had saved is kept and merged.
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

Settings (`config.toml`) do not sync yet.

