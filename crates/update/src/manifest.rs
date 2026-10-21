//! The list of releases, `release.json`: each channel's current release,
//! and every release's files by platform.
//!
//! ```json
//! {
//!   "channels": { "stable": "v0.2.0", "beta": "v0.3.0-beta2" },
//!   "releases": {
//!     "v0.2.0": {
//!       "releaseDate": "2026-11-02",
//!       "archives": { "linux_x86_64": { "file": "…", "sha256": "…" } }
//!     }
//!   }
//! }
//! ```

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::version::{Channel, Version};

#[derive(Debug, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub channels: BTreeMap<String, String>,
    #[serde(default)]
    pub releases: BTreeMap<String, Release>,
}

#[derive(Debug, Deserialize)]
pub struct Release {
    #[serde(rename = "releaseDate", default)]
    pub release_date: String,
    /// What to download for each platform (`linux_x86_64`), when the app
    /// installs it itself.
    #[serde(default)]
    pub archives: BTreeMap<String, Archive>,
}

#[derive(Debug, Deserialize)]
pub struct Archive {
    pub file: String,
    pub sha256: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("the list of releases does not read: {0}")]
    Malformed(String),
    #[error("the list of releases: {0} is not a version such as v1.2.3 or v1.2.3-beta1")]
    Version(String),
    #[error("the list of releases: {release} {platform}: {sha256} is not a SHA-256")]
    Sha256 {
        release: String,
        platform: String,
        sha256: String,
    },
    #[error("the list of releases: {release} {platform}: {file} is not that platform's file")]
    File {
        release: String,
        platform: String,
        file: String,
    },
    #[error("the list of releases: {0} is not a channel")]
    Channel(String),
    #[error("the list of releases: channel {channel} is {release}, which it does not list")]
    Unlisted { channel: String, release: String },
    #[error("the list of releases: channel stable is {0}, a beta")]
    BetaOnStable(String),
}

impl Manifest {
    /// Reads a list and checks it whole: a list that fails any of it is
    /// refused, not partly used.
    pub fn parse(data: &[u8]) -> Result<Manifest, ManifestError> {
        let m: Manifest =
            serde_json::from_slice(data).map_err(|e| ManifestError::Malformed(e.to_string()))?;
        for (name, release) in &m.releases {
            let v = Version::parse(name).ok_or_else(|| ManifestError::Version(name.clone()))?;
            if v.to_string() != *name {
                return Err(ManifestError::Version(name.clone()));
            }
            for (platform, a) in &release.archives {
                // The file the release has for the platform, by its name.
                if crate::platform::archive_file(&v, platform).as_deref() != Some(a.file.as_str()) {
                    return Err(ManifestError::File {
                        release: name.clone(),
                        platform: platform.clone(),
                        file: a.file.clone(),
                    });
                }
                let hex = a.sha256.len() == 64
                    && a.sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
                if !hex {
                    return Err(ManifestError::Sha256 {
                        release: name.clone(),
                        platform: platform.clone(),
                        sha256: a.sha256.clone(),
                    });
                }
            }
        }
        for (channel, name) in &m.channels {
            if channel != "stable" && channel != "beta" {
                return Err(ManifestError::Channel(channel.clone()));
            }
            if !m.releases.contains_key(name) {
                return Err(ManifestError::Unlisted {
                    channel: channel.clone(),
                    release: name.clone(),
                });
            }
            if channel == "stable" && Version::parse(name).is_some_and(|v| v.pre.is_some()) {
                return Err(ManifestError::BetaOnStable(name.clone()));
            }
        }
        Ok(m)
    }

    /// The release an app on `channel` is offered: the stable channel's; for
    /// beta, the newer of beta's and stable's (a release supersedes the betas
    /// before it). An app on stable is never offered a beta.
    pub fn newest(&self, channel: Channel) -> Option<Version> {
        let mut names = vec![self.channels.get("stable")];
        if channel == Channel::Beta {
            names.push(self.channels.get("beta"));
        }
        names
            .into_iter()
            .flatten()
            .filter_map(|n| Version::parse(n))
            .max()
    }

    /// What `current` is offered: the newest of its channel, when newer.
    pub fn offer(&self, current: &Version) -> Option<Version> {
        self.newest(current.channel()).filter(|n| n > current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).expect(s)
    }

    fn list(stable: &str, beta: &str) -> Manifest {
        let json = format!(
            r#"{{"channels": {{"stable": "{stable}", "beta": "{beta}"}},
                "releases": {{"{stable}": {{"archives": {{}}}}, "{beta}": {{"archives": {{}}}}}}}}"#
        );
        Manifest::parse(json.as_bytes()).expect("list")
    }

    #[test]
    fn stable_is_offered_stable_only_and_beta_the_newer_of_both() {
        let m = list("v0.2.0", "v0.3.0-beta2");
        // On stable, a newer beta is no update.
        assert_eq!(m.offer(&v("0.2.0")), None);
        assert_eq!(m.offer(&v("0.1.0")), Some(v("v0.2.0")));
        // On beta, the newer beta.
        assert_eq!(m.offer(&v("0.3.0-beta1")), Some(v("v0.3.0-beta2")));
        assert_eq!(m.offer(&v("0.3.0-beta2")), None);
        // A release after the betas takes a beta up to stable.
        let m = list("v0.3.0", "v0.3.0-beta2");
        assert_eq!(m.offer(&v("0.3.0-beta2")), Some(v("v0.3.0")));
        // Nothing newer than what runs.
        assert_eq!(m.offer(&v("0.4.0")), None);
    }

    #[test]
    fn a_list_that_does_not_hold_together_is_refused() {
        let bad = |json: &str| Manifest::parse(json.as_bytes()).expect_err(json);
        assert!(matches!(bad("{"), ManifestError::Malformed(_)));
        assert_eq!(
            bad(r#"{"releases": {"0.2.0": {}}}"#),
            ManifestError::Version("0.2.0".into())
        );
        assert_eq!(
            bad(r#"{"channels": {"nightly": "v0.2.0"}, "releases": {"v0.2.0": {}}}"#),
            ManifestError::Channel("nightly".into())
        );
        assert!(matches!(
            bad(r#"{"channels": {"stable": "v0.3.0"}, "releases": {"v0.2.0": {}}}"#),
            ManifestError::Unlisted { .. }
        ));
        assert_eq!(
            bad(r#"{"channels": {"stable": "v0.3.0-beta1"}, "releases": {"v0.3.0-beta1": {}}}"#),
            ManifestError::BetaOnStable("v0.3.0-beta1".into())
        );
        assert!(matches!(
            bad(
                r#"{"releases": {"v0.2.0": {"archives": {"linux_x86_64": {"file": "shellwing_0.2.0_linux_x86_64.AppImage", "sha256": "no"}}}}}"#
            ),
            ManifestError::Sha256 { .. }
        ));
        assert!(matches!(
            bad(
                r#"{"releases": {"v0.2.0": {"archives": {"linux_x86_64": {"file": "other.AppImage", "sha256": "no"}}}}}"#
            ),
            ManifestError::File { .. }
        ));
    }

    /// The repository's own list (signed into the `release` branch's
    /// release.signed.json, docs/releasing.md) holds together: CI says so
    /// before it reaches the signing machine.
    #[test]
    fn the_repositorys_list_holds_together() {
        let list = Manifest::parse(include_bytes!("../../../release.json")).expect("release.json");
        let stable = list.newest(Channel::Stable).expect("a stable release");
        let shipped = &list.releases[&stable.to_string()];
        assert!(
            !shipped.archives.is_empty(),
            "the stable release names its files"
        );
    }
}
