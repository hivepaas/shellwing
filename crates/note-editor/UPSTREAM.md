# Upstream: zorite-editor

This crate is [zorite-editor](https://crates.io/crates/zorite-editor)
**0.11.2** (<https://github.com/packetThrower/zorite>, `crates/zorite-editor`),
MIT, Copyright (c) 2026 Will Lehnertz, taken in and changed. This file says
every change: what, where, and why, so that a newer zorite-editor can be taken
in again with each change made again, on purpose.

- **As published:** commit `f1b4d50` ("Take in zorite-editor 0.11.2 as
  crates/note-editor, as it is") holds `src/` exactly as the 0.11.2 crate has it.
  `git diff f1b4d50 -- crates/note-editor/src` shows everything shellwing changed.
- **Not taken:** `examples/demo.rs` (it needs `os-spellcheck`, a sibling crate),
  `API.md`, the crate's own `README.md`. Its `LICENSE` is kept as it is.
- **Dependencies,** pinned: `zorite-markdown =0.10.0` (default features off: its
  `syntax` module), `gpui-bidi =0.1.1`, `unicode-segmentation 1`; `gpui` is the
  workspace's (`gpui-pre =0.3.8`). Upstream takes them from its own workspace.

In the code, every change says `shellwing:` or goes through `crate::obsidian`:

```sh
grep -rn "shellwing:\|crate::obsidian" crates/note-editor/src
```

## Changes

### 1. A note loads as it is

| Where | What |
|---|---|
| `src/lib.rs`, `fn normalize_loaded` | Returns its input. Upstream rewrote `$$` math fences (`zorite_markdown::syntax::normalize_math_fences`) on every load (`with_text`, `set_text`). |

**Why:** a shell's `$$` (`kill $$`, `echo $$`) is not math; a note merely opened
changed, then saved and synced.

### 2. Obsidian's constructs and math off

`src/obsidian.rs` (new) holds two switches, `OBSIDIAN` and `MATH`, both `false`,
and wraps what recognizes those constructs:

| Wrapper in `obsidian.rs` | Upstream call it replaces | Where |
|---|---|---|
| `block_id` | `zorite_markdown::syntax::block_id` | `markdown_syntax.rs` `scan_line`; `element.rs` the badge in the links' hit test; `lib.rs` the click on a badge |
| `embed_line` | `zorite_markdown::syntax::embed_line` | `element.rs` the embed gap and the embed chip (two places); `lib.rs` `embed_overlays` |
| `prefixed_property` | `zorite_markdown::syntax::prefixed_property` | `markdown_syntax.rs` `property_regions` (two places); `element.rs` the property panel |
| `links`, `link_at` (drop `LinkHit::Page`) | `zorite_markdown::syntax::{links, link_at}` | `markdown_syntax.rs` re-exports them from `obsidian` instead; used by `element.rs` and `lib.rs` as `markdown_syntax::links` / `link_at` |

And in `src/markdown_syntax.rs`, guarded by the switches:

| Function | Change |
|---|---|
| `scan_inline` | The `[[wiki-link]]` branch and the `#tag` branch run only with `OBSIDIAN`. |
| `mermaid_blocks` | Empty without `MATH`. |
| `math_regions` (so `math_blocks` too) | Empty without `MATH`. |
| `inline_math_spans` | Empty without `MATH`. |

Tests of what is off are `#[ignore]`d, saying so: in `markdown_syntax.rs`
`inline_math_escapes_and_later_close`, `inline_display_math_double_dollar`,
`inline_math_basic`, `inline_math_multibyte_safe`,
`link_at_hits_every_linkable_construct`, `mermaid_block_extraction`,
`one_line_math_regions`, `property_regions_group_and_skip_code`; in `lib.rs`
`caret_off_marker_line_cases` lost its math align marker case. `obsidian.rs` has
a test that commands, paths and `$$` read as written.

**Why:** in notes about servers `#` begins a comment and `::` is a path, not a
tag and a property; `$` is a shell's; shellwing has no math or mermaid renderer.

### 3. The selection, for the host

| Where | What |
|---|---|
| `src/lib.rs`, `impl EditorState` (before `replace_range`) | Added `selected_range()`, `select(range, cx)`, `is_selecting()`. |

**Why:** shellwing's bar over the selection (copy, insert into the shell, Markdown
formats) reads and sets the selection.

## Taking in a newer zorite-editor

1. Note the version; read its changelog and `API.md` for what changed.
2. On a branch, put its `src/` over this crate's `src/`, keeping `obsidian.rs`
   (upstream has none).
3. Make each change above again, in order; this file says where. A function
   that moved or was renamed: find the call it makes upstream
   (`zorite_markdown::syntax::block_id` and the like) and route it through
   `crate::obsidian` again.
4. `grep` upstream's new code for further uses of `syntax::block_id`,
   `embed_line`, `prefixed_property`, `links`, `link_at`, `wiki_target_display`,
   `normalize_math_fences`, and for `$$` / `[[` / `#` scanning: route them
   through the switches too.
5. Bump `zorite-markdown` (and `gpui-bidi`) to the versions upstream takes, the
   same in `crates/app`; `gpui-pre` the same everywhere.
6. Update this file: the version, the commit holding it as published (take it
   in unchanged first, in a commit of its own), each change's place.
7. `cargo test -p shellwing-note-editor`, and the app's tests; check a note with
   `kill $$`, `# comment`, `std::env`, `[[x]]` opens and saves unchanged.
