# Notes for AI agents

An agent given a host can run commands and read and write files there
([configuration.md](../configuration.md), AI access). What it cannot know is
what the person knows about that host: what it is for, how its service is
restarted, what broke last time. That is in their notes. This note lets an
agent read the notes about the hosts it was given, and add to them, without
reaching anything else the vault keeps.

What stays out, and why:

- **Environments' values, config files' content, passwords and keys.** These
  are secrets. Once an agent has read one, it can print it or send it
  anywhere. An agent never sees a secret; shellwing uses them for it.
- **Snippets.** They are worth giving too, but separately: running one goes
  through the command path and its risky list, not the vault's. That is a
  later note.

## Two rights, per grant

A grant ([`AiGrant`]) says what its client may do on one host: `run`, `read`
and `write` today. Two more, each on its own and **off** unless turned on:

- **Read notes** (`notes_read`): list the notes about this host and read them.
- **Add to notes** (`notes_add`): write a new note about this host, or add to
  the end of one it can read.

Both live in `state/ai.json` with the grant. A grant saved before them loads
with both off. They are rights to the vault, so they are not implied by
*Read files*: a host's files and the person's notes about it are different
things to give. *Give hosts…* shows them under *What it may do*, below *Write
files*. The table of hosts given says them on each grant, beside the others;
as for those, giving the host again changes them.

## Which notes an agent sees

A note reaches a client only through a host that the note is about
([`Note::hosts`]) and that the client holds with *Read notes*, under a grant
that is in force: not paused, not ended, its client not paused.

- **A note about no host never goes out.** A note about no host is the
  person's own: recovery codes, a to-do list. No grant reaches it.
- **A sensitive note never goes out**, not even its title. Its text shows
  masked until the person proves it is them, and an agent cannot do that.
  Marking a note sensitive is how the person keeps a host's note to themself.
- **Other hosts' names are left out.** A note about `web1` and `db1`, read by
  a client that holds `web1` only, lists `web1` alone among its hosts. Its
  text may still name `db1`; the person chose to write that there.
- **Locked, nothing.** While the vault is locked, or shellwing is locked
  ([configuration.md](../configuration.md), Locking shellwing), every notes
  tool is refused, with the reason. The lock hides notes from whoever is at
  the screen, and an agent run from that account is no different.
- **Personal notes only.** Collections hold no notes today. If they do one
  day, a collection's note goes out under the same rule, through the
  collection's host.

A note names its hosts as the host list shows them (a `hosts.toml` host's
name, an `~/.ssh/config` alias), and a grant by what opening the host
connects to (a `hosts.toml` host's `alias`, if it has one, else its name).
The host list's rows hold both, so a note's host is matched to a grant
through them.

## Tools

Five tools, always listed. The server declares `listChanged: false`, so a
client sees them only after it reconnects. Called without the right, a tool
is refused with what to ask the person for, as `run` is refused on a host
given for reading only.

**`list_notes`** `{host?, query?}`: the notes the client may read, newest
change first, at most 200. Each note gives its id, title, category, format
(`markdown` or `plain`), the hosts it is about (only those the client holds),
when it last changed, and its size. `host` keeps those about that host;
`query` keeps those with the words in their title or text, case aside.

**`read_note`** `{id, offset?}`: a note's text, as it is stored, 64 KB at a
time (a note holds 256 KB at most), with where the rest begins. An image
stays the line it is in the text (`![](attachment:<id>)`), for
`read_note_image`.

**`read_note_image`** `{id, image}`: an image in a note the client may read,
`image` being the id in its `attachment:` link: a screenshot of an error, a
diagram of the network. It comes as MCP's image content (`type: image`, the
bytes in base64, `mimeType` `image/png` or `image/jpeg`), as it is kept: 2 MB
at most, scaled to 2560 px on its long edge
([note-images.md](note-images.md)).

- Only an image **the note's text links to**. An id alone opens nothing: an
  image of a note the client cannot read stays out of reach, even if its id
  is known.
- An image that has not come yet from another device says so, as the note's
  window does, and can be asked for again later.
- An image on the web (`![](https://…)`) is never fetched for an agent: that
  tells its server the note was read. A copy the person downloaded is an
  attachment, and goes as the rest do.

**`add_note`** `{hosts, title, text, format?}`: a new note about `hosts`,
each held with *Add to notes*; Markdown unless told `plain`. It gives the
new note's id. A text takes 64 KB at a call.

An agent adds text only. A text with an `attachment:` link is refused: an
agent puts no images in, and a link to another note's image would show that
image where the person did not put it.

**`append_note`** `{id, text}`: adds a blank line and `text` to the end of a
note the client may read, about a host it holds with *Add to notes*. The new
text never goes over the 256 KB a note holds. This is a change made on this
device, so syncing treats it as any edit
([note-conflicts.md](note-conflicts.md)): changed elsewhere meanwhile, both
texts are kept.

- A note **open in its window** is refused, with the reason: the editor holds
  its text, and the two would write over each other. The agent can try again
  once the window is closed.
- A note's title, category, format, hosts and *sensitive* never change, and
  nothing is replaced or deleted. An agent can only add. What it added, the
  person can read and keep, or take out.

## Asking first

Reading waits for no one, as reading files does. Adding follows the grant's
mode, as writing files does:

- **Run freely**: it is added at once.
- **Ask for risky ones** and **Ask every time**: each add waits for the
  person in AI activity, with the note's title and the text to add: *Add
  it* beside *Refuse it*, as a command waits with *Run it*.

The reason adds wait under the default is what notes are for. The person
trusts their notes and acts on them later: they copy a command from a note,
or press *Insert* on its code block to type it into a pane. Text an agent
added, from a page it read or a file on the host, could hold a command the
person would never have typed. They should see it before it goes in.

## What is kept

AI activity keeps each call as it keeps commands: the client, the tool, the
note's title (and the image's id, for an image) and what it gave, by size.
A refused call is kept as well, with why. It keeps no note's text, read or
added: its log (`state/ai-activity.jsonl`) is plain text, and the vault
keeps notes encrypted. While an addition waits for the person, the activity
shows the text as it will go in, in memory only.

**In the note itself**, what an agent wrote starts with a line that
shellwing puts there, not the agent, so that the person and the next agent
reading the note can tell:

```markdown
> Added by Claude Code, 2026-11-25
```

The client's name is as *Settings › AI* has it; the words are in the app's
language. A plain note gets the same line without the `>`. `add_note` puts
it first in the new note, `append_note` before the text it adds.

A line in the text, rather than a field on the note (`by`, sealed and
synced, shown in the note's window): a line needs nothing new from the
vault or from sync, every device shows it as it is, and an export keeps it.
It can be edited away, and that is fine: it says who wrote the text, it is
not proof of it. AI activity is the record.

## A note's text is data

A note's text is the person's, or synced from another of their devices, or
added by an agent. To the agent reading it, it is data, not instructions,
and the tool descriptions say so. Whether the agent heeds that is up to its
client; shellwing cannot police it. What shellwing can do is above: no
secrets in reach, the person's private notes out of reach, and adds that
wait to be seen.

## Building it

In this order, each a PR of its own:

1. The two rights: `AiGrant`'s fields, *Give hosts…*, the table of hosts
   given.
2. `list_notes`, `read_note` and `read_note_image`.
3. `add_note` and `append_note`, with asking first and the activity.

[`AiGrant`]: ../../crates/config/src/state.rs
[`Note::hosts`]: ../../crates/vault/src/lib.rs
