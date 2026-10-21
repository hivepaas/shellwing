# Notes changed on two devices

A note's text is one field. When two devices change it before either has seen
the other's change, merging keeps the later one ([sync.md](sync.md),
Merging) and the other is lost without a word. A note can hold hours of work,
so for a note's text both are kept: the later edit stays the note, and the
other becomes a note of its own beside it. Questions still to settle are
marked **Open**.

## Telling concurrent edits apart

Stamps alone cannot tell. An edit made after seeing another has a later stamp,
but so does one made offline that never saw it. A device's last sync is not
enough either: through a folder that Dropbox or Syncthing syncs, both devices
write, both believe they synced, and the store is left with two files of the
note (a "conflicted copy"); each device then finds its own version already
synced.

So the text says what it was written from. Beside its value and stamp, a
note's text field holds:

- `seen`: for each device, the stamp of the newest edit of this text by that
  device that the value builds on, its own included;
- `by`: the name of the device that wrote it (the computer's host name), for
  the copy's title.

```json
"text": {
  "val": "…",
  "t": "1761039120000-0000-b7e2",
  "by": "laptop",
  "seen": {
    "a41f": "1761038000000-0002-a41f",
    "b7e2": "1761039120000-0000-b7e2"
  }
}
```

Both are sealed with the record, as every field is.

- **Editing** the text: `seen` becomes the old value's `seen`, with the old
  value's own stamp, and this edit's stamp for this device.
- **X has seen Y** when X's `seen` holds Y's device (the end of Y's stamp) at
  Y's stamp or later.
- **Merging** two values:
  - one that has seen the other wins, as now;
  - when neither has seen the other and their texts differ, they are
    **concurrent**: the later stamp wins, as now, and the other is kept as a
    copy (below);
  - the same text, or the same edit, makes no copy.

  Whichever wins, its `seen` becomes the newest of both for each device. The
  result has seen both, so merging it again with either finds nothing new.

This keeps the merge commutative and idempotent: devices agree whatever order
versions come in. With three concurrent edits the latest stays the note, and
the two others become copies, in any order.

**Why the later edit, and not this device's.** Every device must reach the
same note without talking to the others. Keeping each device's own would need
a new stamp to make it win, which the other device would meet as another
concurrent edit, and so on.

## The copy

The device that finds a conflict makes a new note from the losing side's
version of the note:

- **Content:** its title, category, format, hosts, sensitive flag and text.
- **Title:** `<title> (from <device>)`, with the losing text's `by`, in the
  language of the device that makes it ("another device" when `by` is
  unknown).
- **Id:** derived from the note's id and the losing text's stamp, shaped as
  `new_id` (the losing edit's time, then a SHA-256 of both). Every device that
  finds the same conflict makes the same note, so it is merged as one.
- **Stamps:** every field carries the losing text's stamp. A copy the user
  deleted stays deleted when another device finds the conflict later: the
  deletion is newer.
- **Images:** its text refers to the same attachments, so they are kept as long
  as the copy is ([note-images.md](note-images.md), Removal).

It then syncs as any note. `SyncReport::copies` lists the titles of the copies
made in a round, and the window says so: *A note was changed on two devices:
the other version is kept as Disk usage (from laptop).*

## Where it applies

- **Every merge of a note:** a sync round merging this device's version with
  each file of it in the store (a "conflicted copy" left by a sync tool
  included), and a write refused because another device wrote meanwhile.
  With Git, a refused push runs the round again over theirs; a copy made in
  the first is kept, goes out in the second, and is still said.
- **A note open in its window** is held out of sync, as now, and merged when
  the window closes; it shows the merged text the next time it opens.
- **The text only.** The title, category, hosts and flags stay as they are
  merged now, field by field, the later winning. They are short, and the copy
  keeps the losing side's anyway.

Snippets' bodies could be handled the same way later.

## Limits

- **A deletion and an edit made without seeing it.** The deletion still wins
  when it is later ([sync.md](sync.md)). **Open:** keeping such an edit as a
  copy too (a tombstone would need its own `seen`).
- **Texts from before `seen`** count as having seen only themselves. Two such
  texts that differ count as concurrent: at worst one copy too many, never a
  text lost.

## Tests

- **Merging** (`sync.rs`): edits one after another make no copy, in either
  order; concurrent ones make one, and the later stays; the same text makes
  none; three concurrent edits make two copies whatever the order; merging the
  result again finds nothing; texts from before `seen`.
- **Sync, two and three devices through a folder:**
  - both edit offline: both texts survive on both devices, the later as the
    note, the copy made once;
  - both write in the same round and the store holds two files: one copy;
  - a deleted copy stays deleted when a third device syncs an old version;
  - a note held open merges when let go;
  - the images of the losing text are kept.
- **Sync over WebDAV and Git** (`crates/remote`): two devices, and a write
  refused because the other wrote meanwhile (a WebDAV write refused; a Git
  push refused after the copy was made).
- **UI scenario** (`ui_tests/notes.rs`): a note edited in its window while
  another vault on the same folder changes it; when the window closes it
  syncs, the window says so, and both devices hold the note and the copy.
