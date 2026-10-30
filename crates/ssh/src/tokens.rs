//! The `%` tokens OpenSSH expands in the commands of ssh_config.

use crate::{Error, Resolved};

/// `text`, from `what` (`ProxyCommand`, `RemoteCommand`), with the tokens in
/// `takes` expanded: `%h` the host name, `%p` the port, `%r` the user, `%n`
/// the name as given, `%d` this machine's home, `%u` this machine's user. `%%`
/// is a `%` everywhere; any other token is an error, as in OpenSSH.
pub(crate) fn expand(text: &str, hop: &Resolved, what: &str, takes: &str) -> Result<String, Error> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some(t) if takes.contains(t) => match t {
                'h' => out.push_str(&hop.target.host),
                'p' => out.push_str(&hop.target.port.to_string()),
                'r' => out.push_str(&hop.target.user),
                'n' => out.push_str(&hop.alias),
                'd' => out.push_str(
                    &std::env::home_dir()
                        .map(|h| h.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                ),
                _ => out.push_str(&crate::config::local_user()),
            },
            Some(t) => {
                return Err(Error::Config(format!(
                    "{what} {text:?}: %{t} is not a token it takes"
                )));
            }
            None => {
                return Err(Error::Config(format!("{what} {text:?} ends in a lone %")));
            }
        }
    }
    Ok(out)
}
