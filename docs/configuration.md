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

## config.toml

```toml
[appearance]
theme = "neon-nebula"        # a theme id or name, see below; default "default-slate"
font = { family = "JetBrains Mono", size = 14 }
line_height = 1.3            # row height as a multiple of the font size
cursor = { style = "block" } # "block", "bar" or "underline"

[terminal]
scrollback = 10000           # lines of history
```

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

**Changing the theme.** <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>]</kbd> and
<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>[</kbd> (<kbd>Cmd</kbd>+<kbd>]</kbd> and
<kbd>Cmd</kbd>+<kbd>[</kbd> on macOS) step through the themes and save the
choice to `config.toml`. They stand in until the theme picker arrives.

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
auth = { method = "key", key = "~/.ssh/deploy_ed25519" }
theme = "nord"              # beats the group's and config.toml's theme
```

**How `shellwing <name>` finds a host:**

1. A host here with that alias, then with that name.
2. An alias in `~/.ssh/config`.
3. Otherwise `<name>` is used as the host name.

A user or port typed on the command line (`root@api1:22`) wins over the file.

**Fields not used yet.** `id`, `tags` and `local_only` are read but have no
effect yet; they are for sync. The `password` and `agent` methods of `auth`
behave like the default for now: agent, then key files, then a password prompt,
as `ssh` does.

**Theme order,** highest first:

1. The host's `theme`.
2. Its group's `theme`.
3. `config.toml`.
