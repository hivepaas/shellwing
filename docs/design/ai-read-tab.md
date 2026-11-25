# AI agents reading a tab

"Look at the error in my terminal": the person has a tab open, something
went wrong in it, and the agent cannot see it. Copying it into the chat by
hand is what this saves. This note lets an agent read a tab's text, the
screen and what scrolled off it, while the person allows it, for a while.

## Text, not a picture

A terminal keeps its rows as text (libghostty's screen and scrollback), so an
agent reads them as text: exact, small, searchable and quotable. A picture
would cost many times the tokens, and the model would read the letters back
from it, worst on dense logs.

- Rows wrapped by the terminal are joined again into the line the program
  wrote; colors and other escape sequences are left out, as for a command
  typed into a tab.
- A row that was red or bold is not marked. **Later**, if agents miss it: a
  `!` before a line printed in red.

## Allowed per tab, for a while

A tab's screen may hold anything: a token a command printed, a file's
secrets, what the person typed. So:

- **Off unless allowed**, for each pane (a tab split in two has two), for one
  client.
- **For 15 minutes**, then it ends by itself. A pane closed, its tab closed,
  shellwing locked, or the client paused or removed ends it sooner.
- **The person allows it**, either way:
  - from the pane's menu: *Let AI read this pane* › the client;
  - or when an agent asks (below): AI activity shows which pane, with
    *Allow for 15 minutes* and *Refuse it*.
- **Seen while it lasts:** the pane's edge and its tab carry AI's violet mark
  with the time left; a click on it ends it at once.
- **Read only.** Nothing is typed into the pane: typing is what a host given
  *through a tab* does, as before.

It is no grant of a host's: the local terminal's panes may be allowed too,
and a pane to a host the client holds is not readable without being allowed.

## Tools

**`list_tabs`**: the panes open, numbered as the agent names them: for each,
the host (or *local terminal*), which one the person has in front of them
(*focused*), and whether this client may read it now, and until when. A
pane's title is given only while it may be read: a title can say what the
shell is doing.

**`read_tab`** `{tab?, from?, lines?, query?, selection?}`: a pane's text,
`tab` its number, the focused one if not given.

- **As it comes:** the screen, and the 50 lines above it.
- **`from` and `lines`:** a part of the scrollback, `from` the first line (1
  the oldest), `lines` how many: **1000 lines and 128 KB at the most** at a
  call, cut at a whole line. Each answer says how many lines there are in all
  and where the rest begins, as `read_file` does.
- **`query`:** the lines with its words (case aside), each with 2 lines
  around it, 200 matches at the most: a search through the scrollback rather
  than reading it whole.
- **`selection: true`:** what the person selected in the pane, if anything:
  the plainest way to say *this*.
- Each line comes with its number, so the agent can say which and ask for
  more around it.

**Not allowed yet:** `read_tab` asks. It waits for the person as a command
that asks does: allowed, it reads and the pane stays readable for 15
minutes; refused, it says so. A pane in a tab the person cannot see (another
window, a background tab) is asked for the same way.

## What is kept

AI activity keeps each read as it keeps a command: the client, the pane
(its host), and what was read (*lines 3401 to 3460 of 3460*, or the query).
Never the text: the log is plain text, and the screen is the person's.

## What it never shows

- A password typed at a prompt is not echoed, so it is not on the screen.
- An environment's values are sent with the echo off, so they are not
  either. (Checked when this is built: a load or a snippet's run leaves
  nothing of them on the screen.)
- What the person typed at their prompt is on the screen like the rest:
  allowing a pane is allowing that too, and the ask says so.

## Building it

1. Allowing a pane: from its menu and from an ask, the mark with its time,
   the 15 minutes, and what ends it sooner.
2. `list_tabs` and `read_tab`: the screen, parts, `query` and `selection`.
