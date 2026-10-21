# Images in notes

A note can show images: a screenshot of an error, a dashboard, a diagram. They
are kept encrypted in the vault and sync with it, as the note's text does. This
is a decision to build against; questions still to settle are marked **Open**.

## What exists

- **The editor draws block images.** A line holding only `![alt](src)`,
  optionally followed by `{width=N}`, is drawn as the image when the caret is
  elsewhere, with a grip to resize it (which writes `{width=N}`). It asks the
  host for the picture through `EditorState::set_block_image_provider`. The app
  does not install one yet, so such a line shows its source.
- **The clipboard carries images** on macOS, Windows and Linux
  (`ClipboardEntry::Image` in GPUI 0.3.9).
- **The vault keeps binary data**: a config file holds up to 1 MB.

What is missing: somewhere to keep the images, a way to put them in, and the
provider that draws them.

## Where images are kept

Each image is an **attachment**: an immutable, encrypted blob in the vault,
named by a keyed hash of its content. A note refers to it in Markdown:

```markdown
![disk usage after the migration](attachment:4be1c0a7d2f95e13a0c6b8f4e2d71a39){width=640}
```

Ruled out:

| Option | Why not |
| --- | --- |
| Base64 inside the note (`data:` URIs) | A screenshot fills the note's 256 KB at once. The editor would lay out megabytes of text, and every edit of the note would sync the images again. |
| Image files beside `vault.db` | Not encrypted, and not synced. |
| `http(s)://` images | Loading one tells its server when the note is opened, and needs the network. They stay links unless the user loads them (**Open**: an opt-in per note, phase 2). |
| A normal vault record per image | The sync engine reads and merges every record each round; records are assumed small (under 2 KB, [sync.md](sync.md)). Megabytes re-read every minute would be slow on a folder and costly on S3. |

**Naming.** An attachment's id is the first 128 bits of
HMAC-SHA256(attachment key, plaintext), in hex, where the attachment key comes
from the space key through HKDF-SHA256 (info: `shellwing attachment id`).

- The same image pasted twice is stored once.
- A plain SHA-256 would let whoever holds the store test whether it contains a
  known image. A keyed hash does not.
- Content-named and never changed, an attachment cannot conflict: two devices
  that add the same image write the same blob.

**Encryption.** The blob is the image's bytes in the record envelope's sealed
form ([vault.md](vault.md)): cipher id ‖ nonce ‖ AEAD(space key, nonce, bytes,
aad = `attachment` ‖ id ‖ space ‖ key version). Nothing about it is in the
clear but its id and size.

**On this device.** A table in `vault.db`, so an attachment is written in the
same transaction as the note that first refers to it:

```sql
CREATE TABLE attachments (
  id        TEXT PRIMARY KEY,   -- keyed hash, hex
  space     TEXT NOT NULL,
  key_ver   INTEGER NOT NULL,
  data      BLOB NOT NULL,      -- cipher id, nonce, ciphertext and tag
  bytes     INTEGER NOT NULL,   -- plaintext size, for limits and the cache
  mime      TEXT NOT NULL,      -- image/png | image/jpeg
  unused_since INTEGER          -- set when no note refers to it (see Removal)
);
```

## Sync

Attachments sit in their own folder on the backend, apart from the records the
engine merges:

```
shellwing/
  records/<ulid>.sw         merged every round, as today
  attachments/<id>.sw       listed, never merged or read twice
```

- **Pushing.** A device uploads the attachments its notes refer to that the
  listing lacks (`put` with `IfAbsent`), **before** the records that refer to
  them, so no device sees a reference before its image.
- **Pulling.** Attachments are fetched when a note that refers to them arrives,
  in the same round, and only those: a device never downloads images no note
  of its own refers to.
- **Missing.** A note may arrive before its image (another device still
  uploading, or offline mid-way). The line shows a placeholder, *Image not
  synced yet*, and the next round tries again.
- **Healing.** A device that holds an attachment some note refers to, and finds
  it missing from the backend, uploads it again.
- **Listing cost.** `attachments/` is listed each round, as `records/` is; the
  names alone tell what is missing, so nothing is read when nothing is new.
- **Git.** History keeps every image ever pushed, even after it is removed.
  Settings › Sync says so when the backend is Git and the vault holds images
  (**Open**: whether to offer leaving attachments out of Git sync).
- **shellwing cloud.** Attachments count toward the account's storage cap
  ([sync.md](sync.md)), never toward hosts. **Open:** the cap.

## Removal

An attachment is unused when no note's text (deleted notes included, until
their tombstone goes) refers to it.

1. A device notes the time it first finds an attachment unused
   (`unused_since`), and clears it if a note refers to it again (an undo, a
   note brought back by a later edit).
2. After 30 days unused, the device deletes it locally and from the backend.
3. A device still holding a note that refers to it re-uploads it (Healing), so
   a note edited offline across the deletion keeps its image.

Removing an image line from a note does not remove the image at once, so undo
and a note restored from another device keep working.

## Putting images in

All three end the same way: the image is prepared, stored, and a line
`![](attachment:…)` is inserted on a line of its own at the caret.

1. **Paste** (Ctrl+V, Cmd+V on macOS) with an image on the clipboard. The
   common case: a screenshot.
2. **Drop** image files on the note's window.
3. **Insert image…** in the editor's menu and the selection bar: a file picker.

**Preparing an image.**

- Formats read: PNG, JPEG, GIF (its first frame), WebP, BMP.
- **Decompression bombs:** the header's size is checked before decoding; more
  than 50 megapixels is refused.
- **Size:** a long edge over 2560 px is scaled down (Lanczos3). That stays sharp
  on HiDPI screens and keeps a screenshot near a few hundred KB.
- **Format kept:** PNG for images with transparency or few colors
  (screenshots, diagrams), JPEG at quality 85 for the rest (photos). Of the two,
  the smaller is kept when both apply.
- **Metadata** (EXIF, GPS, color profiles' comments) does not survive
  re-encoding: a phone photo does not carry where it was taken.
- **Limits:** 2 MB per attachment after preparing, 20 MB of attachments per
  note. Past either, the image is not added and the editor says why.

## Showing them

- The app installs the block image provider for note windows: `attachment:`
  sources load from `vault.db`, decrypt and decode on a background thread, and
  the provider returns `None` (the line shows its source) until the picture is
  ready, then the note repaints.
- Decoded images are cached, least recently used out, 64 MB in all; a note's
  window keeps its own visible images.
- **Sensitive notes:** images show only once the note's text does, after the
  same check that it is you.
- **Preview** (read-only) draws them the same way; there the grip does not
  resize.
- **Locked vault:** a note's window holds no text, and no images.

## Changes to the editor

`crates/note-editor` is zorite-editor taken in; each change goes into its
`UPSTREAM.md`.

- **Paste:** when the clipboard holds an image and no text, emit
  `EditorEvent::PasteImage(Image)` instead of pasting nothing.
- **Drop:** emit `EditorEvent::DropFiles(paths, offset)` for files dropped on
  the editor.
- **Insert:** a host-supplied line insertion at the caret that keeps undo in
  one step.
- **Image menu** (it exists upstream; read-only today): *Copy image*, *Save
  image as…*, *Remove from note* (the line only). Phase 2.

## Copying and export

- **Copy as Markdown** keeps `attachment:` references, which mean nothing
  outside shellwing. A selection that is a single image puts the image itself
  on the clipboard (phase 2).
- **Export a note** (phase 2): a folder with the note as `note.md`, its images in
  `images/`, and the references rewritten to `images/<name>.png`.

## Phases

1. **Phase 1:** the `attachments` table and backend folder, keyed ids,
   preparing, paste, drop and *Insert image…*, the provider and cache, limits,
   removal, sync (push before records, pull on reference, healing).
2. **Phase 2:** the image menu, copying an image, export, `data:` URIs pasted
   in Markdown turned into attachments, `http(s)` images loaded on request.

## Tests

- **Vault:** keyed ids (same image, same id; another space, another id), seal
  and open with the AAD, refusing a blob moved to another id, the limits,
  unused marking and removal after 30 days.
- **Sync engine:** a folder backend shared by two devices; an image added on
  one shows on the other; pushed before the note; a missing blob healed; a
  removal while another device edits the note offline.
- **Preparing:** scaling, PNG or JPEG chosen as above, EXIF gone, a bomb
  refused before decoding.
- **UI scenarios** (`crates/app/src/ui_tests`): paste a screenshot into a note
  and see it drawn; a second workspace syncing the same folder shows it.

## Threats

| If this happens | Then |
| --- | --- |
| The backend is read by someone else | They see how many images there are and their sizes, not what they show, nor which images they are (the ids are keyed). |
| A crafted image is pasted or synced | Its size is checked before decoding; decoding runs in `image`, off the UI thread. A blob that does not open under its id and AAD is refused. |
| A note is deleted by mistake | Its images stay 30 days, and come back with the note (from another device or an undo). |
