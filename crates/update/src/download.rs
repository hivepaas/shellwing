//! Downloading a release's file. It is kept only once its SHA-256 is the
//! one the signed list names: the list's signatures vouch for the file.

use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::CheckError;

/// A release's file is not bigger than this.
const MOST_FILE: u64 = 512 << 20;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// No byte for this long: the download has stalled.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Downloads `url` to `dest`, telling `progress` the bytes so far and the
/// size when known; `dest` is there only once whole and of `sha256`.
pub async fn download(
    url: &str,
    sha256: &str,
    dest: &Path,
    progress: impl Fn(u64, Option<u64>),
) -> Result<(), CheckError> {
    let fail = |e: &dyn std::fmt::Display| CheckError::Fetch(e.to_string());
    let io = |e: std::io::Error| CheckError::Io(e.to_string());
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .user_agent(concat!("shellwing/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| fail(&e))?;
    let mut res = client.get(url).send().await.map_err(|e| fail(&e))?;
    if !res.status().is_success() {
        return Err(CheckError::Fetch(format!("{url}: {}", res.status())));
    }
    let total = res.content_length();
    if total.is_some_and(|n| n > MOST_FILE) {
        return Err(CheckError::Fetch(format!("{url}: too large")));
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let part = dest.with_extension("part");
    let mut file = std::fs::File::create(&part).map_err(io)?;
    let mut hash = Sha256::new();
    let mut got = 0u64;
    while let Some(chunk) = res.chunk().await.map_err(|e| fail(&e))? {
        got += chunk.len() as u64;
        if got > MOST_FILE {
            let _ = std::fs::remove_file(&part);
            return Err(CheckError::Fetch(format!("{url}: too large")));
        }
        hash.update(&chunk);
        file.write_all(&chunk).map_err(io)?;
        progress(got, total);
    }
    file.sync_all().map_err(io)?;
    drop(file);
    let sum: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if sum != sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(CheckError::Checksum);
    }
    std::fs::rename(&part, dest).map_err(io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use super::*;

    /// Serves `body` once on a port of this machine; its URL.
    fn serve_once(body: &'static [u8]) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().expect("accept");
            let mut req = [0u8; 1024];
            let _ = conn.read(&mut req);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = conn.write_all(head.as_bytes());
            let _ = conn.write_all(body);
        });
        format!("http://{addr}/release")
    }

    fn sha256_of(data: &[u8]) -> String {
        Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[tokio::test]
    async fn a_download_is_kept_only_when_its_hash_is_the_listed_one() {
        let dir = std::env::temp_dir().join(format!("shellwing-download-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dest = dir.join("shellwing.AppImage");
        let body: &[u8] = b"the release";

        let seen = std::sync::Mutex::new(Vec::new());
        download(&serve_once(body), &sha256_of(body), &dest, |n, of| {
            seen.lock().expect("seen").push((n, of));
        })
        .await
        .expect("download");
        assert_eq!(std::fs::read(&dest).expect("read"), body);
        let seen = seen.into_inner().expect("seen");
        assert_eq!(
            seen.last(),
            Some(&(body.len() as u64, Some(body.len() as u64)))
        );

        std::fs::remove_file(&dest).expect("rm");
        let wrong = download(&serve_once(body), &sha256_of(b"another"), &dest, |_, _| {}).await;
        assert!(matches!(wrong, Err(CheckError::Checksum)), "{wrong:?}");
        // Neither the file nor its part is left.
        let left: Vec<_> = std::fs::read_dir(&dir).expect("ls").flatten().collect();
        assert!(left.is_empty(), "{left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
