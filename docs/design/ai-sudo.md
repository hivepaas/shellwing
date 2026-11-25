# sudo for AI agents

An agent runs a command on a connection of its own, with no terminal and no
input ([configuration.md](../configuration.md), AI access). A host whose sudo
asks for a password is closed to it: `sudo -n` fails, and restarting a
service, editing `/etc` or installing a package cannot be done. This note
gives such a command sudo's password, typed or chosen by the person each
time, and never seen by the agent.

## Where it is needed

- **On a connection of its own** (a grant reached in the background): this
  note.
- **Through a tab:** sudo asks in the tab, where the person sees it, and
  they type the password there as they would. Nothing changes.

## Asking for it

`run` takes `sudo: true`. The agent sets it when a command needs sudo's
password, most often after `sudo -n` said *a password is required*: `run`
then says so, and how to ask for one.

Such a run **always waits for the person**, under every mode, *Run freely*
too: AI activity shows the command, with a field for sudo's password and
*Run it* / *Refuse it*.

- **Typed, or the host's saved password.** When the vault keeps a password
  for the host (the one shellwing signs in with), the ask offers *Use the
  saved password*, often sudo's too. Nothing is filled in without a click.
- **Kept a while, never for good.** The ask has *Remember it for 15
  minutes*, unticked. Ticked, the password is kept in memory for that
  client on that host until then, and sudo runs there in that time follow
  the grant's mode, as any command does, rather than each waiting. Untick
  it, and it is forgotten once the run starts, as on *Run on hosts*.
- **How long** is the person's: *Settings › AI*, beside the risky list,
  kept with the other AI settings of this computer (`sudo_minutes` in
  `state/ai.json`, never synced): 15 unless set, at most 480; 0 takes the
  box away, and every sudo run waits. The time runs from when it was allowed, not from
  the last run: a series of commands does not keep it alive.
- **Forgotten sooner:** when shellwing locks, quits or the vault locks, and
  when the grant is paused or taken back, or its client is.
- **Refused or wrong:** a password sudo turns down ends the run *sudo
  refused the password*, and the agent can ask again.

## How it goes in

As *Run on hosts* gives it (`run::invocation`): the password goes first on
the command's input; the command line holds a shell that reads it, then
defines `sudo` as `sudo -A` with `printenv` as its askpass, so only sudo's
own process gets it, from its environment. The command itself is read from
the input after it. The password is never on a command line, in a file, in
what the activity keeps or in what the agent is told.

Only sudo the command calls itself gets it, as on *Run on hosts*: not a
`sudo` inside a script it runs.

## What is kept

AI activity keeps the command as the agent gave it, and that it ran with
sudo's password. Not the password.

## Building it

1. `run`'s `sudo`, the ask with its password field, *Use the saved
   password* and *Remember it for … minutes* (`AiState::sudo_minutes`, in Settings › AI), and the
   input as `run::invocation` makes it.
