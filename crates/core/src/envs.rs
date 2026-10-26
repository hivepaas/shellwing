//! Environments in a session's shell: loaded (their variables kept there),
//! or given to one command (a snippet that needs them). Their values never
//! show: a command that reads them with the echo off is typed, it signals
//! when it is ready ([`shellwing_term::signal_sequence`]), and only then are
//! they sent. What a front end keeps per pane, the desktop's and the phones'.

use std::time::Duration;

use shellwing_i18n::tr;
use shellwing_term::{Mods, NamedKey, TermHandle};
use zeroize::Zeroizing;

use crate::environment;

/// How long the shell has to say it is ready before a load is given up: not
/// a POSIX shell (fish), or not at its prompt. The front end waits this long,
/// then calls [`Envs::give_up`].
pub const WAIT: Duration = Duration::from_secs(15);

/// An environment loaded into the shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedEnv {
    pub id: String,
    pub name: String,
    pub names: Vec<String>,
    /// Its `NAME=value`s, in bytes.
    pub bytes: usize,
}

/// Values waiting to go to the shell: when it signals `ready` (its echo
/// off), and, for one loaded into the shell, `done`.
struct Pending {
    ready: String,
    done: Option<String>,
    values: Vec<Zeroizing<String>>,
    env: Option<LoadedEnv>,
}

/// One shell's environments: loaded, and waiting for its signals.
#[derive(Default)]
pub struct Envs {
    loaded: Vec<LoadedEnv>,
    pending: Vec<Pending>,
}

impl Envs {
    pub fn loaded(&self) -> &[LoadedEnv] {
        &self.loaded
    }

    /// The shell ended: its variables went with it.
    pub fn clear(&mut self) {
        self.loaded.clear();
        self.pending.clear();
    }

    /// Loads `env` (its `values`) into the shell of `term`. Returns the
    /// signal it waits for, to give up on after [`WAIT`].
    pub fn load(
        &mut self,
        term: &TermHandle,
        env: LoadedEnv,
        values: shellwing_vault::EnvValues,
    ) -> Result<String, String> {
        let (names, sent, bytes) = env_values(values)?;
        fits(self.bytes(Some(&env.id)), bytes)?;
        let (ready, done) = (format!("ready-{}", nonce()), format!("done-{}", nonce()));
        term.text(environment::load(&sent, &ready, &done));
        term.key(NamedKey::Enter, Mods::default());
        self.pending.push(Pending {
            ready: ready.clone(),
            done: Some(done),
            values: lines(sent),
            env: Some(LoadedEnv {
                names,
                bytes,
                ..env
            }),
        });
        Ok(ready)
    }

    /// Runs `command` in a subshell that has `values` (a snippet that needs
    /// them): they go when it ends. Returns the signal it waits for.
    pub fn run_with(
        &mut self,
        term: &TermHandle,
        values: shellwing_vault::EnvValues,
        command: &str,
        enter: bool,
    ) -> Result<String, String> {
        let (_, sent, bytes) = env_values(values)?;
        // The subshell has the ones loaded too.
        fits(self.bytes(None), bytes)?;
        let ready = format!("ready-{}", nonce());
        let text = environment::wrapped(&sent, &ready, command.trim_end_matches('\n'));
        // Several lines: one paste, so it runs whole at the Enter.
        term.paste(text, true);
        if enter {
            term.key(NamedKey::Enter, Mods::default());
        }
        self.pending.push(Pending {
            ready: ready.clone(),
            done: None,
            values: lines(sent),
            env: None,
        });
        Ok(ready)
    }

    /// Unloads the environment `id` from the shell (None: all of them).
    pub fn unload(&mut self, term: &TermHandle, id: Option<&str>) {
        let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.loaded)
            .into_iter()
            .partition(|e| id.is_none_or(|id| e.id == id));
        self.loaded = kept;
        let names: Vec<String> = gone.iter().flat_map(|e| e.names.clone()).collect();
        if !names.is_empty() {
            term.text(environment::unload(&names));
            term.key(NamedKey::Enter, Mods::default());
        }
    }

    /// A signal from a command typed here: the values go once the shell is
    /// ready. Returns the name of an environment now loaded.
    pub fn signal(&mut self, term: &TermHandle, signal: &str) -> Option<String> {
        if let Some(i) = self.pending.iter().position(|p| p.ready == signal) {
            for value in std::mem::take(&mut self.pending[i].values) {
                term.text(value.to_string());
                term.key(NamedKey::Enter, Mods::default());
            }
            if self.pending[i].done.is_none() {
                self.pending.remove(i);
            }
            return None;
        }
        let i = self
            .pending
            .iter()
            .position(|p| p.done.as_deref() == Some(signal))?;
        let env = self.pending.remove(i).env?;
        let name = env.name.clone();
        self.loaded.retain(|e| e.id != env.id);
        self.loaded.push(env);
        Some(name)
    }

    /// The shell never said it was ready for `ready`: given up. True when it
    /// was still waiting, to say so.
    pub fn give_up(&mut self, ready: &str) -> bool {
        let before = self.pending.len();
        self.pending.retain(|p| p.ready != ready);
        self.pending.len() < before
    }

    /// The bytes of the environments loaded, but `except`'s.
    fn bytes(&self, except: Option<&str>) -> usize {
        self.loaded
            .iter()
            .filter(|e| except.is_none_or(|x| e.id != x))
            .map(|e| e.bytes)
            .sum()
    }
}

/// What a front end says when [`Envs::give_up`] did.
pub fn gave_up() -> String {
    tr!(
        "The shell did not take the variables: it is not at its prompt, or not a POSIX shell (sh, bash, zsh)."
    )
}

/// The variables to send, each value as its lines; the names too.
fn env_values(
    values: shellwing_vault::EnvValues,
) -> Result<(Vec<String>, Vec<environment::Sent>, usize), String> {
    if values.is_empty() {
        return Err(tr!("It has no variables."));
    }
    // Kept within the limits, whatever device saved it.
    if let Some(o) =
        shellwing_vault::environment_oversize(values.iter().map(|(n, v)| (n.as_str(), v.as_str())))
    {
        return Err(environment::said_oversize(&o));
    }
    let bytes = values.iter().map(|(n, v)| n.len() + 1 + v.len()).sum();
    if let Some((n, _)) = values.iter().find(|(n, _)| !environment::valid_name(n)) {
        return Err(tr!("{name} is not a variable's name.", name = n.clone()));
    }
    let names = values.iter().map(|(n, _)| n.clone()).collect();
    let sent = values
        .iter()
        .map(|(n, v)| environment::encode(n, v))
        .collect();
    Ok((names, sent, bytes))
}

/// Whether `bytes` more fit in the shell beside `loaded`.
fn fits(loaded: usize, bytes: usize) -> Result<(), String> {
    let max = shellwing_vault::limits::LOADED;
    if loaded + bytes <= max {
        return Ok(());
    }
    Err(tr!(
        "With the ones loaded, that comes to {size} KB; a shell takes {max} KB at most: unload one first.",
        size = (loaded + bytes).div_ceil(1024),
        max = max / 1024
    ))
}

/// The lines that carry `sent`'s values, in turn.
fn lines(sent: Vec<environment::Sent>) -> Vec<Zeroizing<String>> {
    sent.into_iter().flat_map(|s| s.lines).collect()
}

/// A word for one load's signals, unlike any other in the pane.
fn nonce() -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{t:x}{n:x}")
}
