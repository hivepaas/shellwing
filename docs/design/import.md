# Importing hosts from other apps

Someone moving to shellwing has their hosts in another SSH client, often
dozens of them in folders. Typing them again is the first reason not to
switch. *Import hosts…* reads them from the other app's files and writes them
into `hosts.toml`, from where they sync as any host does. Questions still to
settle are marked **Open**.

## What phase 1 does, and does not

- **Reads:** hosts, their folders, ports, users, key files, jump hosts,
  forwards, agent forwarding, colors and descriptions.
- **Does not read passwords.** A host asks for its password the first time,
  and the pane offers to keep it in the vault, as it does today.
- **Does not nest groups.** shellwing's groups are one level: a folder
  `Prod › EU` becomes the group `Prod / EU`.
- **Leaves out** what is not SSH (RDP, VNC, telnet, serial), saying so.

## Sources

| App | Found on its own | Or a file chosen | Format |
| --- | --- | --- | --- |
| `~/.ssh/config` | the file `ssh` reads | — | ssh_config |
| PuTTY | Windows: `HKCU\Software\SimonTatham\PuTTY\Sessions`; elsewhere `~/.putty/sessions/` (or `$XDG_CONFIG_HOME/putty/sessions`) | a `.reg` export (`regedit /e`) | registry values; one `Key=Value` file per session |
| KiTTY | `HKCU\Software\9bis.com\KiTTY\Sessions`, or its `Sessions\` folder | a `.reg` export | as PuTTY's |
| MobaXterm | `MobaXterm.ini` in `%APPDATA%\MobaXterm` or `Documents\MobaXterm` | `.mxtsessions` or `.moba` (*Export all sessions to file*), or a portable copy's `MobaXterm.ini` | INI |
| SecureCRT | `Config/Sessions/` under `%APPDATA%\VanDyke\Config`, `~/Library/Application Support/VanDyke/SecureCRT/Config` or `~/.vandyke/SecureCRT/Config` | its XML export (9.6 and later), or a `Sessions` folder | one INI per session; XML |
| Xshell | `Documents\NetSarang Computer\{6,7,8}\Xshell\Sessions`, and Xshell 5's `Documents\NetSarang\Xshell\Sessions` | an `.xts` export (a zip of `.xsh` files) | INI |
| Tabby | `config.yaml` in `%APPDATA%\tabby`, `~/Library/Application Support/tabby` or `~/.config/tabby` (and the older `terminus`) | a `config.yaml` | YAML |
| CSV | — | a `.csv` file: a spreadsheet, Royal TS' export, another app's | header row, then one host per row |

Each found source is listed with the number of hosts in it; one that is not
installed is not listed. A file chosen is recognized by its content, not its
name only.

**Termius** keeps hosts in its cloud and has no export (it imports CSV, but
does not write it), so it cannot be read; the docs say so.

## From each app

All fields are optional except the address. What is not listed is not read.

- **`~/.ssh/config`:** each `Host` name without a pattern (`*`, `?`, `!`):
  `HostName`, `Port`, `User`, the first `IdentityFile`, `ProxyJump`,
  `LocalForward`, `RemoteForward`, `DynamicForward`, `ForwardAgent`. The name
  is also the host's alias, so `shellwing <name>` keeps working. Hosts read
  from `~/.ssh/config` do not sync; imported, they do.
- **PuTTY / KiTTY:**
  - Only sessions with `Protocol` `ssh`; `Default Settings` is left out.
  - Names are `%XX`-escaped in the registry and in file names.
  - Fields: `HostName` (it may hold `user@host`), `PortNumber` (a DWORD),
    `UserName`, `PublicKeyFile`, `AgentFwd`.
  - `PortForwardings` (`L8080=localhost:80,D1080=`, with `\` escaping).
  - A proxy of the SSH kind whose `ProxyHost` names another saved session is
    a jump host.
  - KiTTY's `Folder` is the group.
  - A `.reg` file is UTF-16LE.
- **MobaXterm:**
  - Folders are the `[Bookmarks]` and `[Bookmarks_N]` sections, named by
    their `SubRep` (`Parent\Child`).
  - A session line is `Name=#icon#params#terminal#start#comment#color`, the
    params separated by `%`. The first param is the type: 0 is SSH.
  - Params read: 1 host, 2 port, 3 user (`<default>`: none), 8–10 the jump
    hosts, their ports and users (several hops joined by `__PIPE__`), 14 the
    key file, 34 agent forwarding.
  - Escapes: `__PERCENT__`, `__DIEZE__`, `__PTVIRG__`, `__DBLQUO__`,
    `__PIPE__`, `_ProfileDir_`, `_CurrentDrive_`.
  - The comment is the description. The color is a Windows `0x00BBGGRR`
    number.
  - The file is in the Windows code page: read as UTF-8 when it is valid UTF-8,
    else as GBK when that reads without errors, else as Windows-1252.
- **SecureCRT:**
  - Each `.ini` under `Sessions/`, its folders the group; `Default.ini` and
    `__FolderData__.ini` are left out.
  - Only `S:"Protocol Name"=SSH2`.
  - Fields: `S:"Hostname"`, `D:"[SSH2] Port"` (hex), `S:"Username"`,
    `S:"Identity Filename V2"`, `D:"Enable Agent Forwarding"`.
  - `S:"Firewall Name"=Session:<path>` is a jump host.
  - `Z:"Description"` (several lines) is the description.
  - The XML export holds the same names, its numbers in decimal.
- **Xshell:**
  - Each `.xsh`, its folders the group; Xshell 7 and 8 write UTF-16LE.
  - `[CONNECTION]`: `Host`, `Port`, `Protocol` (`SSH` only).
  - `[CONNECTION:AUTHENTICATION]`: `UserName`. `UserKey` names a key in
    Xshell's own store, not a file, so the host is marked to have its key
    exported from Xshell and added under *Keys*.
- **Tabby:**
  - `profiles` of `type: ssh`.
  - Fields: `name`, `group` (an id into `groups` with their parents, or a name
    in older files), `color`, `options.host`, `port`, `user`, the first
    `file://` of `privateKeys`, `jumpHost` (another profile's id),
    `agentForward`, `forwardedPorts` (`Local`, `Remote`, `Dynamic`).
  - An encrypted `config.yaml` cannot be read without Tabby, and is said to
    be so.
- **CSV:**
  - The header names the columns, in any order, matched without case:
    - `name`, `label` or `title`;
    - `host`, `hostname`, `address` or `ip`;
    - `port`;
    - `user`, `username` or `login`;
    - `group` or `folder`;
    - `tags`;
    - `key` or `identity`;
    - `jump`;
    - `description` or `notes`.
  - Separated by commas, semicolons or tabs (found from the header); quoted as
    RFC 4180 says; UTF-8, with or without its byte order mark.

## Into hosts.toml

| Read | Becomes |
| --- | --- |
| The session's name | `name` (a second host of the same name gets ` (2)`) |
| Its folder path | `group`, the parts joined by ` / `; the group is made if missing |
| Host, port, user | `address`, `port` (left out when 22), `user` |
| A key file | `auth = { method = "key", key = "…" }`, the path as written (`~` kept) |
| Agent forwarding | `forward_agent = true` |
| Jump hosts | `jump`, by the names the jump hosts got in this import; one named but not found is left out, said so; one given by its address (MobaXterm, PuTTY, an ssh config) is the host with that address, port and user, here or imported, or else a host of its own, added |
| Forwards | `forward`, as `ssh` writes them: `-L 8080:localhost:80`, `-R …`, `-D 1080` |
| A color | `color`: one of shellwing's names when close, else `#rrggbb` |
| A description | a Markdown note titled after the host, with the host set on it |
| — | `tags = ["imported:<app>"]` |

A PuTTY `.ppk` key file is kept as the host's key path, and the host is
marked: shellwing reads OpenSSH keys, so the key needs converting (`puttygen
key.ppk -O private-openssh`). Converting it here is phase 2.

## Before writing: the preview

The import shows what it found before anything is written:

- The hosts, by group, each with a checkbox (all checked).
- **Already here:** a host whose address, port and user match one in
  `hosts.toml` is unchecked and marked so. Importing the same app twice adds
  nothing.
- A host whose name is taken by another host gets ` (2)`, shown.
- **Left out**, with why: not SSH, no address, an encrypted file.
- **To look at:** a jump host not found, a `.ppk` key, a key in Xshell's
  store.

*Import* then writes every checked host and its group in **one** edit of
`hosts.toml`, keeping its comments as the host editor does, and the notes in
the vault. The notice says what was done: *Imported 42 hosts in 5 groups; 3
left out.*

**Undo.** Until the app quits, *Undo import from <app>* in the palette
removes the hosts, groups and notes that import made, and nothing else (by
their ids), in one edit. Imported hosts also keep their `imported:<app>` tag,
to find them later.

## Where it lives

- **`crates/import`**, no UI:
  - one module per source, each with where the app keeps its sessions on this
    computer (`installed()` lists them, with their counts), and what is in a
    source (`read()`, or `read_file()` for one chosen by hand, told by its
    content): `Found` hosts (with their folder path, jump references, forwards
    and description), what was left out and why, and what to look at;
  - `plan()`: those hosts as `hosts.toml`'s, against the hosts there (names,
    groups, jump names, duplicates).
- **`shellwing-config`:** `Store::import` writes the groups and hosts in one
  edit, and `Store::unimport` takes them out.
- **The app:** *Import hosts from another app…* in the palette, in the
  empty host list and in Settings › General (the drawer then shows over
  Settings); the drawer (as the host editor's) with the sources and the
  preview; the notes; *Undo import from <app>*.
- **New dependencies:**
  - `zip` (deflate only), for Xshell's `.xts`;
  - a YAML reader, for Tabby.

  `encoding_rs`, `quick-xml`, `winreg` and `ssh2-config` are in the build
  already.

## Privacy

The files are read on this computer and nothing leaves it. Password fields are
never read into memory as passwords: the parsers skip them. The tests use made
up files, never real ones.

## Phases

1. **Phase 1:** `~/.ssh/config`, CSV, PuTTY and KiTTY, MobaXterm, SecureCRT,
   Xshell, Tabby; the preview, *Import*, *Undo import*.
2. **Phase 2:** FinalShell, WindTerm, Electerm, mRemoteNG, WinSCP, Remmina; a
   `.ppk` key converted into the vault.

Saved passwords are not imported in any phase.

## Tests

- **Each source** (`crates/import`), from made-up files in its real format:
  - a plain host;
  - a folder in a folder;
  - a jump host and a chain of them;
  - forwards of each kind;
  - escapes (`%XX`, `__PIPE__`, `\,` in forwards);
  - UTF-16LE, GBK and Windows-1252 files;
  - what is not SSH;
  - an encrypted Tabby file.
- **Into hosts.toml:**
  - names made unique;
  - groups made once;
  - jump names followed;
  - a host already there unchecked;
  - one edit keeping the file's comments;
  - a second import adding nothing.
- **UI scenario** (`ui_tests/hosts.rs`): a MobaXterm export chosen in the file
  dialog, the preview with a host already there, *Import*, the host list
  showing the groups, *Undo import* taking them away.
