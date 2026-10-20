//! Telling that a newer shellwing is out, as the HivePaaS CLI tells it of
//! itself: a signed list of releases (`release.signed.json`, on the
//! repository's `release` branch) says each channel's newest; the app on
//! stable is offered stable releases only, the app on beta the newer of the
//! betas and stable (a release supersedes the betas before it). The list is
//! believed only when its signatures verify against the keys built in.
//!
//! Installing what is offered comes later: for now the app says where to get
//! it, or what updates it when something else installed it.

pub mod envelope;
pub mod install;
pub mod manifest;
pub mod version;

use std::time::Duration;

pub use envelope::{PublicKeys, SignatureError};
pub use install::{Install, install_of};
pub use manifest::{Manifest, ManifestError};
pub use version::{Channel, Version};

/// What the list is signed for: a signature for another of the keys'
/// purposes (HivePaaS's own release, its CLI's) is never taken for this one.
pub const CONTEXT: &str = "shellwing-release-v1";

/// Where the signed list is read: the release branch, which moves to a
/// release once it is checked.
pub const LIST_URL: &str =
    "https://raw.githubusercontent.com/hivepaas/shellwing/release/release.signed.json";

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

/// The release keys this app trusts (HivePaaS's, see keys/README.md).
pub fn trusted_keys() -> Result<PublicKeys, SignatureError> {
    envelope::parse_keys(&[
        ("2026_ed", include_str!("../keys/2026_ed.pub.pem")),
        ("2026_ml", include_str!("../keys/2026_ml.pub.pem")),
    ])
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
