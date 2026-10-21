# Design notes

How shellwing stores, syncs and protects data, as agreed in October 2026. These
are decisions to build against, not descriptions of code that exists: none of
it is implemented yet. Questions still to settle are marked **Open**.

| Note | Covers |
| --- | --- |
| [config.md](config.md) | Files on disk, their formats, settings precedence |
| [import.md](import.md) | Hosts read from other SSH clients into `hosts.toml` |
| [note-conflicts.md](note-conflicts.md) | A note changed on two devices: both texts kept |
| [note-images.md](note-images.md) | Images in notes: attachments, their sync and removal |
| [sync.md](sync.md) | Accounts, the sync engine, backends, host limits |
| [teams.md](teams.md) | Organizations, sharing, the self-hosted server |
| [vault.md](vault.md) | Secrets, the key hierarchy, encryption, local storage |

Principles shared by all of them:

- **Works without an account.** Signing in only adds shellwing's own cloud sync
  and team features.
- **End-to-end encrypted sync.** Servers and storage backends hold ciphertext.
  Signing in proves who you are; it never unlocks data.
- **One sync engine.** shellwing cloud, a self-hosted server, Git, S3 and a
  plain folder are all backends of the same engine.
- **OpenSSH stays the reference.** `~/.ssh/config` and `~/.ssh/known_hosts` are
  read as `ssh` reads them; `~/.ssh/config` is never written.
