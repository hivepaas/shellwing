# Files between this computer and a host, for AI agents

An agent reads and writes a host's files through `read_file` and
`write_file`: what it reads comes into its context, and what it writes goes
out of it, 512 KB and 8 MB at most, in base64 when not text. To put a build
or a dump on a host, or bring a log back, the bytes go through the model:
slow, costly, and too small. Most agents (Claude Code, Codex CLI, Cursor) run
on the same computer as shellwing, so shellwing can copy a file between a
path here and a path there itself, and the agent is told only what was
copied.

## The danger, and the folders

A path on this computer is the person's: `~/.ssh/id_ed25519`, the vault, a
browser's cookies. An `upload` that took any of them would send a key to a
host, the one thing an agent is never to have. So an agent reaches only the
folders the person gave it:

- **Local folders**, per client, in *Settings › AI*: none until the person
  adds one (a project, `~/Downloads`). With none, `upload` and `download`
  are refused, and say how to give one.
- A path is taken whole (`..` and links followed) before it is checked: it
  must be inside one of them.
- **Never**, whatever the folders: `~/.ssh`, `~/.gnupg`, shellwing's own
  configuration and vault, the system keychain's files. A folder given that
  holds one of them is not refused, but that part of it is.

## Rights and asking

- `upload` writes on the host: it needs *Write files*, and waits for the
  person as a write does (where risky ones wait, every upload does).
- `download` writes on this computer: it needs *Read files* on the host, and
  waits the same way, since it adds or replaces a file of the person's.
- The ask shows both paths, the size, and whether a file there is replaced.

## Tools

**`upload`** `{host, local_path, remote_path, mode?}`: copies a file or a
folder here to the host over SFTP. `mode` as `write_file`'s: `create` (one
there already is an error), `overwrite` (the default). A path on the host
as `write_file` takes it (`~` the home folder).

**`download`** `{host, remote_path, local_path, mode?}`: the other way.

Both are jobs, as `run` is: a large one goes on while the agent follows it
with `job_output`, which says how much has been copied, and `job_stop` (or
*Stop* in AI activity) ends it.

**Folders** are copied whole, file by file over SFTP, so the host needs
nothing but its SFTP server: folders made and files copied. Links in a
folder uploaded are left out, never followed, so a link inside a folder
given reaches nothing outside it. A folder that holds a path never reached
(above) is not uploaded at all.

**No limit on size.** A copy streams, so it takes as little memory for 40 GB
as for 4 KB. What stands in for a limit:

- The ask shows the whole size (a folder's files counted first), and the
  person allows it or not.
- A file downloaded that would not fit in the free space here is refused
  before it starts. (A folder's size, and a host's free space, are not known
  beforehand.)
- Written beside the target first (a file, or a whole folder), then renamed
  over it, as config files are placed: a copy stopped or broken leaves
  nothing half-written, and what was there stays.

The agent is told the paths, the size and the time it took. Not a byte of a
file.

## What is kept

AI activity keeps each copy: its direction, both paths and its size.

## Building it

1. *Local folders* per client, and the path check (with the paths never
   reached).
2. `upload` and `download` of a file, as jobs, with asking first and the
   free space looked at.
3. Whole folders.
