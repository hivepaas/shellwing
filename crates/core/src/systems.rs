//! Which system a host runs, for its tile in the host list: one command,
//! run beside the shell once the host is connected, at most once a day,
//! and only while the host's icon is left to it.

use shellwing_config::SystemSeen;

/// What is run on the host: its `os-release`, then the kernel's name and
/// release. It writes nothing and asks for nothing.
pub const COMMAND: &str =
    "cat /etc/os-release 2>/dev/null; echo; echo \"uname=$(uname -sr 2>/dev/null)\"";

/// How long what was seen holds before the host is asked again, whatever
/// the answer was.
pub const RECHECK_SECS: u64 = 24 * 60 * 60;

/// Whether to look again, `now` in seconds since 1970.
pub fn due(seen: Option<&SystemSeen>, now: u64) -> bool {
    seen.is_none_or(|s| now.saturating_sub(s.checked_at) >= RECHECK_SECS)
}

/// The system icon for what [`COMMAND`] printed: from the `ID` of
/// `os-release`, else the first of its `ID_LIKE` the app has an icon for,
/// else the kernel's name. None when it is none of those.
pub fn parse(out: &str) -> Option<&'static str> {
    let field = |key: &str| {
        out.lines().find_map(|l| {
            let v = l.trim().strip_prefix(key)?.strip_prefix('=')?;
            Some(
                v.trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_lowercase(),
            )
        })
    };
    let uname = field("uname").unwrap_or_default();
    // Proxmox VE says Debian in its os-release; its kernel tells it apart.
    if uname.starts_with("linux ") && uname.contains("-pve") {
        return Some("proxmox");
    }
    if let Some(id) = field("ID")
        && let Some(icon) = by_id(&id)
    {
        return Some(icon);
    }
    if let Some(like) = field("ID_LIKE")
        && let Some(icon) = like.split_whitespace().find_map(by_id)
    {
        return Some(icon);
    }
    match uname.split_whitespace().next()? {
        "linux" => Some("linux"),
        "darwin" => Some("macos"),
        "freebsd" => Some("freebsd"),
        _ => None,
    }
}

/// The icon for an `os-release` id.
fn by_id(id: &str) -> Option<&'static str> {
    Some(match id {
        "ubuntu" => "ubuntu",
        "debian" => "debian",
        "fedora" => "fedora",
        "rhel" => "redhat",
        "centos" => "centos",
        "rocky" => "rocky",
        "almalinux" => "alma",
        "alpine" => "alpine",
        "arch" | "archarm" => "arch",
        "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "sles" | "suse" => "opensuse",
        "kali" => "kali",
        "linuxmint" => "mint",
        "raspbian" => "raspberry-pi",
        "freebsd" => "freebsd",
        "nixos" => "nixos",
        "openwrt" => "openwrt",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(release: &str, uname: &str) -> String {
        format!("{release}\n\nuname={uname}\n")
    }

    #[test]
    fn takes_the_id_first() {
        let ubuntu =
            "PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nNAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian";
        assert_eq!(
            parse(&out(ubuntu, "Linux 6.8.0-45-generic")),
            Some("ubuntu")
        );
        let rocky = "NAME=\"Rocky Linux\"\nID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"";
        assert_eq!(parse(&out(rocky, "Linux 5.14.0")), Some("rocky"));
        let alpine = "NAME=\"Alpine Linux\"\nID=alpine\nVERSION_ID=3.20.3";
        assert_eq!(parse(&out(alpine, "Linux 6.6.0")), Some("alpine"));
        let leap = "NAME=\"openSUSE Leap\"\nID=\"opensuse-leap\"\nID_LIKE=\"suse opensuse\"";
        assert_eq!(parse(&out(leap, "Linux 6.4.0")), Some("opensuse"));
    }

    #[test]
    fn falls_back_to_what_it_is_like() {
        let pop = "NAME=\"Pop!_OS\"\nID=pop\nID_LIKE=\"ubuntu debian\"";
        assert_eq!(parse(&out(pop, "Linux 6.9.3")), Some("ubuntu"));
    }

    #[test]
    fn then_to_the_kernel() {
        // Amazon Linux is like CentOS and Fedora, but is neither: Linux.
        let amzn = "NAME=\"Amazon Linux\"\nID=\"amzn\"";
        assert_eq!(parse(&out(amzn, "Linux 6.1.0")), Some("linux"));
        assert_eq!(parse(&out("", "Darwin 24.1.0")), Some("macos"));
        assert_eq!(parse(&out("", "FreeBSD 14.1-RELEASE")), Some("freebsd"));
        assert_eq!(parse(&out("", "")), None);
        // A shell that is not sh: nothing it printed makes sense.
        assert_eq!(parse("'cat' is not recognized as a command"), None);
    }

    #[test]
    fn proxmox_by_its_kernel() {
        let pve = "PRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\nID=debian";
        assert_eq!(parse(&out(pve, "Linux 6.8.12-4-pve")), Some("proxmox"));
        assert_eq!(parse(&out(pve, "Linux 6.1.0-26-amd64")), Some("debian"));
    }

    #[test]
    fn looks_again_after_a_day() {
        assert!(due(None, 1_000));
        let seen = SystemSeen {
            system: None,
            checked_at: 1_000,
        };
        assert!(!due(Some(&seen), 1_000 + RECHECK_SECS - 1));
        assert!(due(Some(&seen), 1_000 + RECHECK_SECS));
    }
}
