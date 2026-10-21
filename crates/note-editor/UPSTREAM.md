# Upstream: zorite-editor

This crate is [zorite-editor](https://crates.io/crates/zorite-editor)
**0.11.2** (<https://github.com/packetThrower/zorite>, `crates/zorite-editor`),
MIT, Copyright (c) 2026 Will Lehnertz, taken in and changed. This file says
every change: what, where, and why, so that a newer zorite-editor can be taken
in again with each change made again, on purpose.

- **As published:** `scripts/upstream_diff.sh zorite-editor 0.11.2 crates/note-editor`
  fetches the 0.11.2 crate from crates.io and shows everything shellwing
  changed in `src/`. (It was first taken in unchanged, at `f1b4d50`; PRs are
  squash-merged, so that commit is not on main.)
- **Not taken:** `examples/demo.rs` (it needs `os-spellcheck`, a sibling crate),
  `API.md`, the crate's own `README.md`. Its `LICENSE` is kept as it is.
- **Dependencies,** pinned: `zorite-markdown =0.10.0` (default features off: its
  `syntax` module), `gpui-bidi =0.1.1`, `unicode-segmentation 1`; `gpui` is the
  workspace's (`gpui-pre =0.3.9`). Upstream takes them from its own workspace.

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

### 4. Formats for the host's bar

| Where | What |
|---|---|
| `src/lib.rs`, before `enum TurnKind` | Added `pub enum InlineFormat` (bold, italic, strike, code, underline, highlight) and `pub enum BlockKind` (the "Turn into" kinds without math), with `BlockKind::turn` / `BlockKind::of` to and from `TurnKind`. |
| `src/lib.rs`, `impl EditorState` (before `turn_into`) | Added `toggle_inline(f, cx)` (calls `toggle_wrap` / `toggle_wrap_pair` as the `bold`, `italic`, `strike`, `code`, `underline`, `highlight` actions do), `turn_block(kind, window, cx)` (calls `turn_into`; back to text when the block is that kind already), `block_at_caret()` (from `block_kind_at`). |
| `src/lib.rs`, the "Turn into" flyout (`TurnKind::ALL` in `render`) | Leaves out `TurnKind::Math` while `obsidian::MATH` is off. |

Test: `the_hosts_formats_go_on_and_come_off` in `lib.rs`'s tests.

**Why:** shellwing's bar over the selection formats as the editor's own keys and
menu do, rather than with rules of its own that disagreed (`_` against `*`, a
fenced block of the selection against the caret's block).

### 5. The host's items in the menu, and the menu on a selection

| Where | What |
|---|---|
| `src/lib.rs`, `enum EditorEvent` | Added `HostItem(SharedString)`: one of the host's items was chosen. |
| `src/lib.rs`, before `InlineFormat` | Added `pub struct HostItem { id, label, enabled }`. |
| `src/lib.rs`, `struct EditorState` and `new` | Added the fields `host_items` and `menu_on_select`. |
| `src/lib.rs`, `impl EditorState` | Added `set_host_items(items, cx)` and `set_menu_on_select(on)`. |
| `src/lib.rs`, `on_mouse_up` | With `menu_on_select`, a selection made with the mouse (a drag, a double or triple click) opens the menu 10 px under where the button was let go, as a right click in it does. The parameter `_` became `event`. |
| `src/lib.rs`, `move_to` and `replace_text_in_range` (the input handler's) | Close the menu: a caret moved, or typing over the selection. |
| `src/lib.rs`, `render`, the menu (`self.menu`) | `host_rows` (the host's items, the link color, faint when not enabled; a press emits `HostItem`) before `format_bar`, with a divider; `link_c` taken from the style beside `hover`. The format bar, the color rows and "Turn into" show only with a Markdown style (`markdown`): plain text has none of it. |

**Why:** one menu over a selection, not the editor's and shellwing's own:
shellwing puts *Insert into …* (the shell) first, and the menu shows on
selecting, not only on a right click.

### 6. Read-only

| Where | What |
|---|---|
| `src/lib.rs`, `struct EditorState` and `new` | Added the field `read_only`. |
| `src/lib.rs`, `impl EditorState` | Added `set_read_only(on, cx)` (closes the menus, drops a composition) and `is_read_only()`. |
| `src/lib.rs`, `render` | The actions that edit (`backspace`, `delete`, `newline`, `bold`, `italic`, `underline`, `strike`, `highlight`, `code`, `indent`, `outdent`, `paste`, `cut`, `undo`, `redo`) are registered only while editable (`.when(!self.read_only, …)`, with `use gpui::prelude::FluentBuilder`); the moves, selection, `select_all`, `copy`, `dismiss` always. |
| `src/lib.rs`, `on_mouse_down` | While read-only: no image resize (`grip_at`), no language picker on a code card (Copy still copies), no callout fold flip (`alert_fold_at`, it rewrites the source), no table `+` (`table_add_row_at`, `table_add_col_at`), no row/column delete (`table_row_del`, `table_col_del`), no column resize. A task's box still ticks. |
| `src/lib.rs`, `on_right_mouse_down` | While read-only: no image menu, no table menu: the plain menu. |
| `src/lib.rs`, `render`, the menu | While read-only: no Cut, no Paste; `markdown` (format bar, colors, Turn into) is off too. |
| `src/element.rs`, both `caret_row` and `selection` (prepaint, and the measuring before it) | Not while read-only: no reveal on the caret's line, as when unfocused. |
| `src/element.rs`, `paint` | No `handle_input` while read-only (no typing, no IME); no caret painted. |
| `src/element.rs`, the gutter grip | Not while read-only (no block drag). |

**Why:** a note's reading view is the editor itself, read-only, so the text
stays where it is between reading and editing (zorite-markdown, the reader,
lays out otherwise and selects nothing); it selects and copies.

### 7. A code block's Insert

| Where | What |
|---|---|
| `src/lib.rs`, `enum EditorEvent` | Added `InsertCode(SharedString)`: a code block's body. |
| `src/lib.rs`, `struct EditorState` and `new` | Added the field `code_insert: Option<SharedString>` (the button's label). |
| `src/lib.rs`, `impl EditorState` | Added `set_code_insert(label, cx)`. |
| `src/lib.rs`, `struct CodeChipHit` | Added `insert: Option<Bounds<Pixels>>`. |
| `src/lib.rs`, `struct CodeChip` | Added `insert: Option<(SharedString, Bounds<Pixels>, Hitbox)>`. |
| `src/lib.rs`, new `enum ChipPart` (`Lang`, `Copy`, `Insert`) | `code_chip_at` returns it instead of `(bool, usize)`'s bool. |
| `src/lib.rs`, `on_mouse_down`, the code card branch | `ChipPart::Insert` emits `InsertCode` with the block's body. |
| `src/element.rs`, the code card chrome (prepaint) | With a label, an Insert pill left of Copy; the language pill moves left of it. |
| `src/element.rs`, the code card chrome (paint) | Paints the Insert pill as the others; `CodeChipHit.insert`. |

**Why:** a command in a note's code block goes into the shell with one press.

### 8. Wide tables scroll where they can be seen

| Where | What |
|---|---|
| `src/element.rs`, the wide table's scroll thumb (prepaint) | 6 px high (was 3), 5 px above the table's bottom, the border's color at 0.9 alpha (was its own tint × 1.5, at most 0.8). |
| `src/element.rs`, the thumb's paint | Corner radius 3 px (was 1.5). |
| `src/tables.rs`, `on_scroll_wheel` | With Shift, a vertical wheel scrolls the table under the pointer sideways (`shifted`), and the page does not move. |

**Why:** the thumb was hard to see, and a mouse without a sideways wheel had no
way along a wide table but that thumb.

### 9. A quick drag selects all the way; the menu opens under the selection

| Where | What |
|---|---|
| `src/element.rs`, `paint` (before the read-only `handle_input`) | While `is_selecting`, a window-level mouse-move listener (capture phase) moves the selection's end as `on_mouse_move` does (`table_offset_at`, else `index_for_mouse_position`): the editor's own listener hears the mouse only over it, and a quick drag left it first. |
| `src/lib.rs`, `on_mouse_up` (change 5's menu on a selection) | The menu's anchor is under the selection's head (`bounds_for_offset(cursor_offset())`, 6 px below), not where the button was let go. |

**Why:** a quick drag stopped short; let go outside the editor, the menu sat at
the window's edge.

### 10. The host's items at the caret; the menu's Paste as the keys'

| Where | What |
|---|---|
| `src/lib.rs`, `pub struct HostItem` | Added `selection: bool`: an action on the selection (shown only with one), or something put in at the caret. |
| `src/lib.rs`, `render`, the menu | The host's items are built by `host_section`. Those with `selection` stay at the menu's top (`host_rows`), only with a selection; the others go under Paste (`caret_rows`, a divider above them), and not while read-only. |
| `src/lib.rs`, `render`, the menu's Paste | Dispatches the `Paste` action (`window.dispatch_action`) instead of calling `paste` itself, so a paste the editor does not take (an image on the clipboard) goes on to the host, as it does from the keys. |

**Why:** shellwing's *Insert Image…* goes in at the caret, with nothing
selected, beside Paste as another way to put something in, and never into a
read-only note; and an image pasted from the menu did nothing, since `paste`
passed it on (`cx.propagate()`) outside an action's dispatch.

### 11. The host's items in an image's menu; an offset for a drop

| Where | What |
|---|---|
| `src/lib.rs`, `enum EditorEvent` | Added `ImageItem { item, src }`: one of the host's items in an image's menu was chosen, with the image's `src`. |
| `src/lib.rs`, `struct EditorState` and `new` | Added the field `image_items`. |
| `src/lib.rs`, `impl EditorState` | Added `set_image_items(items, cx)`, and `offset_for_position(position)`: the byte offset under a point, as a click would place the caret (`index_for_mouse_position`). |
| `src/lib.rs`, `on_right_mouse_down` | The image menu opens read-only too when the host has items for it (was: never read-only, change 6). |
| `src/lib.rs`, `render`, the image menu | The host's items first (`image_src` of the row, a press emits `ImageItem`), a divider, then Delete, which stays out while read-only. |

**Why:** shellwing's *Copy Image* and *Save Image As…*, in Preview too; and
images dropped on a note go where they were dropped.

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
