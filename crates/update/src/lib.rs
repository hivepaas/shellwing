//! Telling that a newer shellwing is out: a signed list of releases
//! (`release.signed.json`, on the repository's `release` branch, made by
//! `tools/releasesign`) says each channel's newest; the app on
//! stable is offered stable releases only, the app on beta the newer of the
//! betas and stable (a release supersedes the betas before it). The list is
//! believed only when its signatures verify against the keys built in.
//!
//! A copy the app can replace (an AppImage, an `.app`, one its installer put
//! on Windows) downloads the release's file for its platform, checks it
//! against the list's SHA-256, and installs it as it quits (`apply`). Any
//! other copy is told where the release is, or what updates it.

pub mod apply;
pub mod download;
pub mod envelope;
pub mod install;
pub mod manifest;
pub mod platform;
pub mod version;

use std::time::Duration;

pub use envelope::{PublicKeys, SignatureError};
pub use install::{Install, install_of};
pub use manifest::{Manifest, ManifestError};
pub use version::{Channel, Version};

/// What the list is signed for: a signature made with the keys for another
/// purpose is never taken for this one.
pub const CONTEXT: &str = "shellwing-release-v1";

/// Where the signed list is read: the release branch, which moves to a
/// release once it is checked. A build that tries the updater
/// (scripts/update_try.sh) names its own, as it is built.
pub const LIST_URL: &str = match option_env!("SHELLWING_UPDATE_LIST") {
    Some(url) => url,
    None => "https://raw.githubusercontent.com/hivepaas/shellwing/release/release.signed.json",
};

/// The list is small; anything bigger is not it.
const MOST_LIST: usize = 1 << 20;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// The page a release is downloaded from.
pub fn release_page(v: &Version) -> String {
    format!("https://github.com/hivepaas/shellwing/releases/tag/{v}")
}

/// The version running.
pub fn current() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version {
        major: 0,
        minor: 0,
        patch: 0,
        pre: None,
    })
}

// KEYS: every keys/<id>.pub.pem, by id (build.rs).
include!(concat!(env!("OUT_DIR"), "/keys.rs"));

/// This build has release keys: it can tell of updates. One without (a
/// build before the keys were made) does not look.
pub fn has_keys() -> bool {
    !KEYS.is_empty()
}

/// The release keys this app trusts (keys/README.md), or the test's.
pub fn trusted_keys() -> Result<PublicKeys, SignatureError> {
    #[cfg(feature = "test-source")]
    if let Some((_, keys)) = test_source() {
        let pems: Vec<(&str, &str)> = keys.iter().map(|(i, p)| (i.as_str(), p.as_str())).collect();
        return envelope::parse_keys(&pems);
    }
    envelope::parse_keys(KEYS)
}

/// Where the signed list is read: [`LIST_URL`], or the test's.
pub fn list_url() -> String {
    #[cfg(feature = "test-source")]
    if let Some((url, _)) = test_source() {
        return url;
    }
    LIST_URL.to_string()
}

/// A list and keys of a test's own: the url, and each key's id and PEM.
#[cfg(feature = "test-source")]
type Source = (String, Vec<(String, String)>);

#[cfg(feature = "test-source")]
static TEST_SOURCE: std::sync::Mutex<Option<Source>> = std::sync::Mutex::new(None);

#[cfg(feature = "test-source")]
fn test_source() -> Option<Source> {
    TEST_SOURCE.lock().ok()?.clone()
}

/// Reads the signed list at `url` and checks it with `keys` (id, PEM), in
/// place of the release's: for the app's UI tests.
#[cfg(feature = "test-source")]
pub fn set_test_source(url: &str, keys: &[(&str, &str)]) {
    if let Ok(mut s) = TEST_SOURCE.lock() {
        *s = Some((
            url.to_string(),
            keys.iter()
                .map(|(i, p)| (i.to_string(), p.to_string()))
                .collect(),
        ));
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("{0}")]
    Fetch(String),
    /// No list is published yet (before the first release): nothing to offer.
    #[error("no release is published yet")]
    NoList,
    #[error(transparent)]
    Signature(#[from] SignatureError),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("the download is not the file the list of releases names: its SHA-256 differs")]
    Checksum,
    #[error("{0}")]
    Io(String),
    #[error("the release has no file for this platform")]
    NoFile,
}

/// A release's file for this platform, as the signed list names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseFile {
    pub version: Version,
    pub file: String,
    pub sha256: String,
    pub url: String,
}

/// The file `version` has for this platform, from a signed list.
pub fn release_file(envelope: &[u8], version: &Version) -> Result<ReleaseFile, CheckError> {
    let data = envelope::open(&trusted_keys()?, envelope, CONTEXT)?;
    let m = Manifest::parse(&data)?;
    let release = m
        .releases
        .get(&version.to_string())
        .ok_or(CheckError::NoFile)?;
    let a = platform::current()
        .and_then(|p| release.archives.get(p))
        .ok_or(CheckError::NoFile)?;
    Ok(ReleaseFile {
        version: version.clone(),
        url: platform::download_url(version, &a.file),
        file: a.file.clone(),
        sha256: a.sha256.clone(),
    })
}

/// What a signed list offers the app at `current`: None when it is the
/// newest of its channel.
pub fn offer(envelope: &[u8], current: &Version) -> Result<Option<Version>, CheckError> {
    let data = envelope::open(&trusted_keys()?, envelope, CONTEXT)?;
    Ok(Manifest::parse(&data)?.offer(current))
}

/// Reads the signed list from `url`.
pub async fn fetch(url: &str) -> Result<Vec<u8>, CheckError> {
    let fail = |e: &dyn std::fmt::Display| CheckError::Fetch(e.to_string());
    // reqwest on rustls with ring, as the sync places use it. Once is
    // enough; again is harmless (the first one installed stays).
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(concat!("shellwing/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| fail(&e))?;
    let res = client.get(url).send().await.map_err(|e| fail(&e))?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(CheckError::NoList);
    }
    if !res.status().is_success() {
        return Err(CheckError::Fetch(format!(
            "the list of releases: {}",
            res.status()
        )));
    }
    if res.content_length().is_some_and(|n| n as usize > MOST_LIST) {
        return Err(CheckError::Fetch(
            "the list of releases is too large".into(),
        ));
    }
    let body = res.bytes().await.map_err(|e| fail(&e))?;
    if body.len() > MOST_LIST {
        return Err(CheckError::Fetch(
            "the list of releases is too large".into(),
        ));
    }
    Ok(body.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repository's signed list (published on the `release` branch,
    /// docs/releasing.md) opens with the keys built into the app, under its
    /// context, and carries release.json byte for byte: a list changed and
    /// not signed again fails here.
    #[test]
    fn the_signed_list_opens_with_the_apps_keys() {
        let keys = envelope::parse_keys(KEYS).expect("the app's keys");
        let signed = include_bytes!("../../../release.signed.json");
        let opened = envelope::open(&keys, signed, CONTEXT).expect("signed by the app's keys");
        assert_eq!(
            opened.as_slice(),
            include_bytes!("../../../release.json").as_slice()
        );
        manifest::Manifest::parse(&opened).expect("a list the app takes");
    }
}
