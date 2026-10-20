//! A release's version, as its tag says it: `v0.2.0`, or `v0.3.0-beta1` for a
//! beta (no dot before the number). The `v` may be
//! left out: the app's own, from Cargo, has none.

use std::cmp::Ordering;
use std::fmt;

/// The channel a release is on: a beta's version has its name and number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
}

impl Channel {
    pub fn name(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// A pre-release's name and number: `beta`, 1. None for a release.
    pub pre: Option<(String, u64)>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix('v').unwrap_or(s);
        let (core, pre) = match s.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (s, None),
        };
        let mut parts = core.split('.');
        let mut number = || -> Option<u64> {
            let p = parts.next()?;
            (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
                .then(|| p.parse().ok())
                .flatten()
        };
        let (major, minor, patch) = (number()?, number()?, number()?);
        if parts.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => None,
            Some(p) => {
                let name: String = p.chars().take_while(char::is_ascii_lowercase).collect();
                let n = &p[name.len()..];
                if name.is_empty() || n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                Some((name, n.parse().ok()?))
            }
        };
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }

    pub fn channel(&self) -> Channel {
        if self.pre.is_some() {
            Channel::Beta
        } else {
            Channel::Stable
        }
    }
}

impl fmt::Display for Version {
    /// `v0.3.0-beta1`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some((name, n)) = &self.pre {
            write!(f, "-{name}{n}")?;
        }
        Ok(())
    }
}

impl Ord for Version {
    /// A pre-release comes before its release: v1.0.0-beta2 < v1.0.0; betas
    /// by their number, not as text (beta10 after beta9).
    fn cmp(&self, o: &Version) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(o.major, o.minor, o.patch))
            .then_with(|| match (&self.pre, &o.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Version) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).expect(s)
    }

    #[test]
    fn versions_read_as_tags_and_as_cargo_writes_them() {
        assert_eq!(v("v0.2.0"), v("0.2.0"));
        assert_eq!(v("0.3.0-beta2").to_string(), "v0.3.0-beta2");
        assert_eq!(v("v1.0.0").channel(), Channel::Stable);
        assert_eq!(v("v1.0.0-beta4").channel(), Channel::Beta);
        for bad in [
            "",
            "1.2",
            "1.2.3.4",
            "v1.2.x",
            "1.2.3-beta",
            "1.2.3-Beta1",
            "1.2.3-1",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_beta_comes_before_its_release_and_betas_count() {
        assert!(v("v1.0.0-beta2") < v("v1.0.0"));
        assert!(v("v1.0.0-beta9") < v("v1.0.0-beta10"));
        assert!(v("v0.9.9") < v("v1.0.0-beta1"));
        assert!(v("v0.10.0") > v("v0.9.0"));
    }
}
