# shellwing-note-editor

The editor of shellwing's notes. It is
[zorite-editor](https://crates.io/crates/zorite-editor) 0.11.2
(<https://github.com/packetThrower/zorite>), MIT-licensed, Copyright (c) 2026
Will Lehnertz: taken into this repository so that shellwing reviews, tests and
changes it as its own, and does not wait on another project for a fix. Its
license is in [LICENSE](LICENSE).

## Changed from zorite-editor 0.11.2

- **Nothing changes a note's text on load.** zorite-editor rewrote `$$` math
  fences when it loaded a document; shell commands hold `$$` (`kill $$`), so a
  note merely opened could change, then save and sync.
- **Obsidian's constructs are off** (`OBSIDIAN` in `lib.rs`): `[[wiki-links]]`,
  `#tags`, `^block-ids`, `key:: value` properties, `![[embeds]]`, `$…$` and
  `$$…$$` math, mermaid. In notes about servers, `#` and `::` are comments and
  paths, not tags and properties.
- The host can read and set the selection (`selected_range`, `select`) for its
  own bar.

Every change, where it is and why, and how to take in a newer zorite-editor:
[UPSTREAM.md](UPSTREAM.md).

Taken out next: the code behind what is off, once nothing reaches it; then tests
for the editing core (`element.rs` has none).
