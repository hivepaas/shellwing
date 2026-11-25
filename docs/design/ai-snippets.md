# Snippets for AI agents

An agent given a host can run any command there. What it does not have is
the commands the person has already written and checked for that host:
their snippets ([configuration.md](../configuration.md), Snippets). This
note lets an agent list the snippets offered for a host it holds and run
one, through the same path as `run`, and suggest a new one.

What stays out, and why:

- **The environments' values.** A snippet that needs environments runs, but
  the agent never sees what they hold: shellwing sends the values itself
  (Environments, below), and every such run waits for the person.
- **The snippet library.** Its items are not the person's until they add
  one. An agent sees the snippets in the vault only.
- **Changing snippets.** An agent does not edit or delete one. It may
  suggest a new one (`add_snippet`, below), which the person allows first.

## A right, per grant

**Use snippets** (`snippets`): list the snippets offered for this host, and
run one. **Off** unless turned on, as for notes ([ai-notes.md](ai-notes.md)):
a snippet is kept in the vault, encrypted, and its command may hold what the
person would not hand out (a token typed into it, a server's name). Running
one is running a command, so `run_snippet` also needs *Run commands*; with
that off, the right lets an agent list snippets only.

It lives in `state/ai.json` with the grant, and a grant saved before it has
it off. *Give hosts…* shows it under *What it may do*.

## Which snippets an agent sees

Those the palette offers in a pane of that host: a snippet for every host,
for the host's group, or for that host ([`Scope::fits`]), from the person's
own vault. The host's group is the host list's, as a pane has it.

## Tools

**`list_snippets`** `{host}`: the snippets offered for `host`, by category
then name. Each gives its id, name, category, its command (several lines as
they are), the values it asks for (its `{{name}}`s other than `host`, `user`
and `address`), and whether it needs environments or is typed without Enter.

**`run_snippet`** `{host, id, values?, wait_seconds?}`: the snippet, its
`{{name}}`s filled in, run on `host` as `run` runs a command: on a
connection of its own, or typed into its tab where the grant says so. It
answers as `run` does: the output and exit status, or a job to follow with
`job_output`.

- `host`, `user` and `address` are filled in from the host, as in a pane.
- Every other `{{name}}` takes its value from `values`. One missing is
  refused, with the names it needs.
- The command, once filled in, takes 64 KB at most, as a snippet does.

**`add_snippet`** `{host, name, command, category?}`: suggests a snippet for
one host the client holds with *Use snippets*, never for every host or a
group, which would offer it on hosts not given. It needs no environment
(an agent cannot attach one) and is typed without Enter, so in a pane the
person reads it before it runs, and an agent's `run_snippet` of it waits
for them (Asking first). It is kept with `origin` `ai:<client>`, and
Settings and the palette say so beside it: *from Claude Code*. Its command
takes 64 KB at most, its name 200 characters.

## Values an agent gives

In a pane, the person types each value and sees the command before it runs.
An agent's values come from the model, and one of them may come from a page
it read. So a value is taken only if it cannot change what the command is:

- letters, digits and `_ . / : @ = + , % -`, nothing else: no space, no
  quote, no `$`, `;`, `|`, `&`, `<`, `>`, `` ` ``, `\`, `*`, `?`, `~`, `#`,
  `!`, brackets or newline;
- not starting with `-`, which a program would take as an option;
- 256 characters at most.

These characters mean the same unquoted, inside double quotes and inside
single quotes, so a value fills a `{{name}}` wherever the snippet puts it,
and stays one word. A value outside them is refused with the reason, and
the agent can use `run` instead, where the person sees the whole command.

Quoting each value instead was ruled out: a snippet often quotes its
`{{name}}` already (`grep "{{pattern}}" app.log`), and a value quoted again
inside would be searched for with its quotes.

## Environments

A snippet's environments are loaded for it alone, as in a pane, and the
agent never sees their values:

- **On a connection of its own,** the values go in on the command's input,
  as `run.rs` sends sudo's password: the command line holds the names only,
  and a few lines of `sh` read the values (written as `environment.rs`
  writes them for `read`, `\0ooo` for what is not printable ASCII), export
  them in a subshell, and run the snippet there. They are gone when it ends.
- **Typed into a tab,** where the grant says so, as a pane does it: the
  echo off, the values sent once the shell says it is ready.

Every such run waits for the person, under every mode, *Run freely* too:
it reads like any other run, but the person is told which environments it
loads. What the command prints still goes to the agent, so a command that
prints a value hands it over; the person sees the command first, and can
refuse it.

## Asking first

The filled-in command goes through what `run` goes through: the grant's
mode and the risky list, matched against the command as it will run, not
the snippet as written. A snippet allowed for the session is allowed by its
command, as any command is.

Two more wait for the person under every mode, *Run freely* too:

- a snippet typed **without Enter**: its author wanted to see it before it
  runs, every time. A snippet an agent suggested is one;
- a snippet that **needs environments** (above).

`add_snippet` follows the grant's mode, as adding to a note does: under
*Ask for risky ones* and *Ask every time* it waits for the person, who sees
its name, host and command, with *Add it* and *Refuse it*.

## What is kept

AI activity keeps a snippet's run as a command: the command as it ran, then
what it printed. Its label is the snippet's name, beside the command. A
listing is kept by host and count, as notes are.

## Building it

In this order, each a PR of its own:

1. The right: `AiGrant::snippets`, *Give hosts…*, the table of hosts given,
   and the host's group in the hub.
2. `list_snippets` and `run_snippet`, for snippets that need no
   environment.
3. Snippets that need environments: their values on the input, or typed
   into the tab.
4. `add_snippet`, and *from Claude Code* beside a snippet an agent
   suggested.

[`Scope::fits`]: ../../crates/core/src/snippets.rs
