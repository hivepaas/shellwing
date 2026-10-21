# Images in notes

A note can show images: a screenshot of an error, a dashboard, a diagram. They
are kept encrypted in the vault and sync with it, as the note's text does.
Phases 1 and 2 below are built. Questions still to settle are
marked **Open**.

## What the editor brings

- **It draws block images.** A line holding only `![alt](src)`, optionally
  followed by `{width=N}`, is drawn as the image, with a grip to resize it
  (which writes `{width=N}`). It asks the host for the picture through
  `EditorState::set_block_image_provider`, and for a label to show instead
  through `set_block_chip_provider` (asked first).
- **It passes on pastes it does not take.** With an image or files on the
  clipboard and no text, the editor's `Paste` propagates to the window.
- **The clipboard carries images** on macOS, Windows and Linux
  (`ClipboardEntry::Image` in GPUI 0.3.9).

So phase 1 needed no change to the editor (`crates/note-editor`). Phase 2
added the host's items in an image's menu and an offset for a drop
(`UPSTREAM.md`, change 11), and items under Paste (change 10).

## Where images are kept

Each image is an **attachment**: an immutable, encrypted blob in the vault,
under a random id. A note refers to it in Markdown:

```markdown
![](attachment:019a4be1c0a7d2f95e13a0c6b8f41a2c){width=640}
```

Ruled out:

| Option | Why not |
| --- | --- |
| Base64 inside the note (`data:` URIs) | A screenshot fills the note's 256 KB at once. The editor would lay out megabytes of text, and every edit of the note would sync the images again. |
| Image files beside `vault.db` | Not encrypted, and not synced. |
| `http(s)://` images, loaded when the note opens | Loading one tells its server who opened the note, and when, every time (a 1×1 pixel is enough to track); a note may be someone else's; it needs the network, and the picture can change after it was written about. They are downloaded only when asked, and kept (see Images on the web). |
| A normal vault record per image | The sync engine reads and merges every record each round; records are assumed small (under 2 KB, [sync.md](sync.md)). Megabytes re-read every minute would be slow on a folder and costly on S3. |

**Ids.** An attachment's id is made as a record's (`new_id`: time, then 80
random bits, in hex). A keyed hash of the content was considered, to store an
image pasted twice once, but the key would come from the space key, which
changes when a device joins another vault: every id would change, and every
note's text with it. Random ids reveal nothing about the image and never
change; an image pasted twice is kept twice.

**Encryption.** The blob is the image's bytes sealed as a record is
([vault.md](vault.md)): cipher id ‖ nonce ‖ AEAD(space key, nonce, bytes,
aad = `attachment` ‖ id ‖ space ‖ key version). The AAD binds a blob to its id:
one moved to another id does not open. Only PNG and JPEG are kept; their type
is read from the bytes, so nothing about an image is in the clear but its id
and size.

**On this device.** A table in `vault.db`:

```sql
CREATE TABLE attachments (
  id            TEXT PRIMARY KEY,
  space         TEXT NOT NULL,
  key_ver       INTEGER NOT NULL,
  data          BLOB NOT NULL,   -- cipher id, nonce, ciphertext and tag
  bytes         INTEGER NOT NULL, -- plaintext size, for the limits
  unused_since  INTEGER          -- ms; set while no live note refers to it
);
```

**Joining another vault** seals every attachment again with that vault's key,
in the same transaction as the records (`adopt`).

## Sync

Attachments sit in their own folder on the backend, apart from the records the
engine merges:

```
shellwing/
  records/<id>.sw         merged every round
  attachments/<id>.sw     listed every round, never merged or read twice
```

Each round (`sync_round`), around the records:

1. **Listing.** `attachments/` is listed; the names alone tell what the store
   holds, so a round with nothing new reads no image.
2. **Pushing, before the records.** The attachments the notes here refer to
   that the store lacks are written (`replace`: a create-only write where the
   backend has one, as `If-None-Match: *` on WebDAV and S3). A note's record
   is never written before its images, so no device finds a reference before
   its image. This also puts back an image that went missing from the store.
3. **Records** merge as before.
4. **Pulling.** The attachments the merged notes refer to that are not here
   are read, checked (size, opens under its id and this vault's key, is a PNG
   or JPEG) and kept as they came. One the store lacks (another device still
   sending it) or that does not check out counts as missing; the next round
   tries again. A device fetches only the images its notes show.
5. **Removal** (below).

`SyncReport` counts `images_pushed`, `images_pulled` and `images_missing`.

The WebDAV and S3 backend keeps every record it read in memory, to answer the
next round without a download; images are left out of that cache, and a
refused create of an image does not download it.

**Git.** History keeps every image ever pushed, even after it is removed, so
the repository only grows. Settings › Sync says so under Git, before it is
chosen; images are not left out of Git sync, since a note would then show
them on one device only.

**shellwing cloud.** Attachments count toward the account's storage cap
([sync.md](sync.md)), never toward hosts. **Open:** the cap.

## Removal

An attachment is unused while no live note's text refers to it. Deleted notes
cannot be looked into (a tombstone keeps no text), so they do not count.

1. A device marks an attachment unused when it first finds it so
   (`unused_since`), and clears the mark if a note refers to it again (an
   undo, a note brought back by a later edit).
2. After 30 days unused, the device removes it here and from the store.
3. A device whose note still refers to it puts it back (pushing, above), so a
   note edited offline across the removal keeps its image.

Removing an image's line from a note does not remove the image at once, so
undo and a note restored from another device keep working. A vault that does
not sync looks after its own with `Vault::sweep_attachments`.

## Putting images in

All three end the same way: each image is prepared, kept, and a line
`![](attachment:…)` goes in place of the selection, on a line of its own (a
line break is added before or after where text would share it).

1. **Paste** (Ctrl+V, Cmd+V on macOS) with an image on the clipboard (a
   screenshot), or image files copied in a file manager.
2. **Drop** image files on the note's window: where they were dropped, at the
   start of the line under the pointer, or after it when the pointer is in a
   line (a line of text is never split).
3. **Insert Image…** in the editor's menu, under Paste (not while
   read-only): a file dialog, several files at once.
4. **Markdown with `data:` images** (pasted from elsewhere): each
   `data:image/…;base64,…` becomes an attachment, and its reference replaces
   the URI.

Images go into Markdown notes only, while they are edited (not in Preview);
otherwise the window says why.

**Preparing an image** (`shellwing_core::images::prepare`, off the UI thread):

- Formats read: PNG, JPEG, GIF (its first frame), WebP, BMP, TIFF (macOS puts
  screenshots on the clipboard as TIFF).
- **Decompression bombs:** the header's size is checked before decoding; more
  than 50 megapixels is refused, and the decoder is held to that size.
- **Size:** a long edge over 2560 px is scaled down (Lanczos3). That stays
  sharp on HiDPI screens and keeps a screenshot near a few hundred KB.
- **Format kept:** PNG when the image has transparency; otherwise PNG unless
  it is more than twice the size of the same image as JPEG at quality 85.
  Screenshots and diagrams stay crisp PNG; photos become JPEG.
- **Metadata** (EXIF, GPS) does not survive re-encoding: a phone photo does
  not carry where it was taken.
- **Limits:** 2 MB per attachment once prepared, 20 MB of attachments per
  note. Past either, the image is not added and the window says why.

GPUI already takes in the `image` crate with these formats, so the app does
not grow.

## Showing them

- The note's window installs the editor's two providers (`note_images.rs`):
  - the label provider answers *Loading the image…*, *Image not synced yet* or
    *The image cannot be shown* while an attachment is not drawn, and asks for
    it to be loaded the first time;
  - the image provider answers with the decoded picture once it is ready.
- Loading reads the attachment from the vault and decodes it (to BGRA, as GPUI
  paints) on a background thread; the editor repaints when it is ready.
- Decoded images are kept per window, 64 MB in all, the oldest dropped first
  (and dropped from the GPU's atlas).
- An image not synced yet is looked for again every second, and drawn once a
  sync brought it.
- **Sensitive notes:** images show only once the note's text does.
- **Locked vault:** a note's window holds no text and forgets its images.

## Images on the web

A line `![alt](https://…)` is never fetched by itself: it shows a label,
*Image from example.com*, and a click opens it in the browser. The note's
toolbar offers *Download Images* (with their count) while it has any:

1. It asks first, naming the hosts that will be contacted (they learn the
   user's address), and saying so when some come over plain `http` (they
   could be altered on the way; the picture is still checked as any is).
2. Each is downloaded once, off the UI thread, prepared and kept as an
   attachment, as a pasted image is; its link becomes `attachment:<id>`
   (a `{width=N}` after it stays). From then on it syncs encrypted, shows
   offline, and its server is never asked again.

Downloading is held to the internet: a host whose name resolves to this
computer, a private or link-local network or carrier-grade NAT (IPv4 and
IPv6) is refused, said why, and not contacted, so a note cannot make the app
reach into the user's network (a router's page, a cloud's metadata address).
The connection goes to the addresses checked (`resolve_to_addrs`), so a name
that answers otherwise the second time leads nowhere else; a redirect is
checked the same way, five at most. 20 MB, 10 s to connect and 30 s in all,
`reqwest` on rustls, as the sync places and the update check have it.

## An image's menu, and export

- Right-click an image: *Copy Image* (the image itself on the clipboard),
  *Save Image As…* (a file dialog, Downloads first; the note's title as the
  name), then *Delete image* while edited. In Preview the first two only.
- *Export…*, beside the note's Delete: asks first, since the files leave the
  vault unencrypted ([vault.md](vault.md): plain-text export needs
  confirmation), then writes a folder named after the note, in one the user
  chooses: `note.md`, its images in `images/<id>.png|jpg`, the references
  rewritten to them. Images not synced yet are left out, and said so.

## Phases

1. **Phase 1 (built):** the `attachments` table and backend folder, preparing,
   paste, drop and *Insert Image…*, the providers and cache, limits, removal,
   sync (push before records, pull on reference, putting back), sealing again
   on joining.
2. **Phase 2 (built):** the image's menu (*Copy Image*, *Save Image As…*),
   *Export…* with `images/`, `data:` URIs pasted in Markdown turned into
   attachments, dropping at the pointer, web images downloaded when asked.

## Tests

- **Vault** (`attachments.rs`): refs found in Markdown; kept sealed and given
  back; only PNG and JPEG, 2 MB; a blob under another id does not open;
  unused ones go after 30 days, and one used again is kept.
- **Sync** (`sync.rs`): an image goes with its note, written before it, and
  nothing is sent or fetched again; a missing image is put back; a damaged one
  is not taken; joining another vault keeps the images; one no note shows for
  30 days goes from the store; no 16 bytes of an image are in the clear in any
  file, the store's or either device's `vault.db`. Over WebDAV (`crates/remote`): an image travels,
  and is read once.
- **Preparing** (`images.rs`): a screenshot stays PNG, a photo becomes JPEG,
  transparency stays PNG, large images are scaled, EXIF and GPS are gone, a
  bomb is refused before decoding, what is not an image is refused.
- **UI scenarios** (`ui_tests/notes.rs`): a screenshot pasted into a note is
  kept and drawn; the note saved and opened again draws it from the vault;
  *Insert Image…* with a file; a copied script is refused, saying why; a plain
  text note says images need Markdown; *Copy Image*; *Save Image As…*; a drop
  in a line goes after it; a pasted `data:` URI becomes an attachment;
  *Export…* writes `note.md` and `images/`; a web image is not fetched by
  itself; one on a private network is refused, the server never asked; one
  downloaded comes once, its link replaced, its width kept.
- **Web images** (`note_images.rs`): links found once each, hosts read,
  private addresses (IPv4, IPv6, mapped) refused, `localhost` refused before
  any connection.

## Threats

| If this happens | Then |
| --- | --- |
| The backend is read by someone else | They see how many images there are and their sizes, not what they show. |
| A crafted image is pasted or synced | Its size is checked before decoding, and decoding runs in `image`, off the UI thread. A blob that does not open under its id and this vault's key is not taken. |
| A note is deleted by mistake | Its images stay 30 days, and come back with the note (from another device or an undo). |
| A note links images to track who reads it, or to reach the reader's network | Nothing is fetched unless asked; asking names the hosts; private networks are refused, after a redirect too, and the address checked is the one connected to. |
