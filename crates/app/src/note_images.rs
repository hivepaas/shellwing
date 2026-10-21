//! Images in a note's window (`docs/design/note-images.md`): put in by
//! pasting, dropping or choosing files, kept in the vault as attachments, and
//! shown by the editor, which asks for them by their `attachment:<id>` source.
//! Loading and decoding run off the UI thread; until an image is ready, or
//! when it cannot be, its line shows a label instead.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{RenderImage, SharedString};
use shellwing_core::images::{PrepareError, prepare};
use shellwing_i18n::tr;
use shellwing_note_editor::EditorState;
use shellwing_vault::{Oversize, Vault, attachment_line, attachment_refs, limits};

/// Decoded images a window keeps, together, at most.
const CACHE_BYTES: usize = 64 * 1024 * 1024;
/// A file read to be an image holds this much at most: anything larger is
/// not a picture to keep in a note.
const READ_AT_MOST: u64 = 64 * 1024 * 1024;
const SCHEME: &str = "attachment:";

enum Slot {
    /// Asked for; being read and decoded.
    Loading,
    /// Decoded, with its size in memory.
    Ready(Arc<RenderImage>, usize),
    /// Not in the vault yet: another device has not sent it.
    Missing,
    /// It would not open or decode.
    Failed,
}

/// The images a note's window shows.
#[derive(Default)]
pub struct Images {
    slots: HashMap<String, Slot>,
    /// Ready ones, the oldest first.
    order: VecDeque<String>,
    bytes: usize,
    /// Let go of, for the window to drop from its atlas.
    evicted: Vec<Arc<RenderImage>>,
}

pub type SharedImages = Rc<RefCell<Images>>;

/// What became of loading an image.
pub enum Loaded {
    Image(Arc<RenderImage>),
    Missing,
    Failed,
}

impl Images {
    /// Takes in what loading `id` came to.
    pub fn put(&mut self, id: String, loaded: Loaded) {
        let slot = match loaded {
            Loaded::Image(image) => {
                let s = image.size(0);
                let size = (s.width.0.max(0) as usize) * (s.height.0.max(0) as usize) * 4;
                self.bytes += size;
                self.order.push_back(id.clone());
                Slot::Ready(image, size)
            }
            Loaded::Missing => Slot::Missing,
            Loaded::Failed => Slot::Failed,
        };
        self.slots.insert(id, slot);
        // The oldest go first, past the window's share; the newest stays.
        while self.bytes > CACHE_BYTES && self.order.len() > 1 {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            if let Some(Slot::Ready(image, size)) = self.slots.remove(&old) {
                self.bytes -= size;
                self.evicted.push(image);
            }
        }
    }

    /// The images not in the vault yet, to look for again.
    pub fn missing(&self) -> Vec<String> {
        self.slots
            .iter()
            .filter(|(_, s)| matches!(s, Slot::Missing))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Forgets `id`: asked for again the next time it shows.
    pub fn forget(&mut self, id: &str) {
        if let Some(Slot::Ready(image, size)) = self.slots.remove(id) {
            self.bytes -= size;
            self.order.retain(|o| o != id);
            self.evicted.push(image);
        }
    }

    /// Forgets them all (the vault locked).
    pub fn clear(&mut self) {
        for (_, slot) in self.slots.drain() {
            if let Slot::Ready(image, _) = slot {
                self.evicted.push(image);
            }
        }
        self.order.clear();
        self.bytes = 0;
    }

    #[cfg(all(test, feature = "ui-tests"))]
    pub fn ready_count(&self) -> usize {
        self.slots
            .values()
            .filter(|s| matches!(s, Slot::Ready(..)))
            .count()
    }

    pub fn take_evicted(&mut self) -> Vec<Arc<RenderImage>> {
        std::mem::take(&mut self.evicted)
    }
}

/// The attachment `src` names, from an image line's `attachment:<id>`.
pub fn attachment_id(src: &str) -> Option<&str> {
    let id = src.trim().strip_prefix(SCHEME)?;
    (id.len() >= 16 && id.bytes().all(|b| b.is_ascii_hexdigit())).then_some(id)
}

/// Lets `editor` show attachments: drawn once they are ready, a label
/// meanwhile. An image not asked for yet goes to `wanted`, to load.
pub fn show_in(
    editor: &mut EditorState,
    images: SharedImages,
    wanted: async_channel::Sender<String>,
) {
    let drawn = images.clone();
    editor.set_block_image_provider(move |src| {
        let id = attachment_id(src)?;
        match drawn.borrow().slots.get(id) {
            Some(Slot::Ready(image, _)) => Some(image.clone()),
            _ => None,
        }
    });
    editor.set_block_chip_provider(move |src| {
        // On the web: never fetched by itself; a click opens it in the
        // browser, Download Images keeps a copy.
        if (src.starts_with("https://") || src.starts_with("http://"))
            && let Some(host) = host_of(src)
        {
            return Some(tr!("Image from {host}", host = host).into());
        }
        let id = attachment_id(src)?;
        let mut all = images.borrow_mut();
        let label: SharedString = match all.slots.get(id) {
            Some(Slot::Ready(..)) => return None,
            Some(Slot::Loading) => tr!("Loading the image…").into(),
            Some(Slot::Missing) => tr!("Image not synced yet").into(),
            Some(Slot::Failed) => tr!("The image cannot be shown").into(),
            None => {
                all.slots.insert(id.to_string(), Slot::Loading);
                let _ = wanted.try_send(id.to_string());
                tr!("Loading the image…").into()
            }
        };
        Some(label)
    });
}

/// Reads and decodes attachment `id` (off the UI thread).
pub fn load(vault: &Vault, id: &str) -> Loaded {
    match vault.attachment(id) {
        Ok(Some(bytes)) => decode(&bytes).map_or(Loaded::Failed, Loaded::Image),
        Ok(None) => Loaded::Missing,
        Err(_) => Loaded::Failed,
    }
}

/// An image file's bytes, as GPUI paints them (BGRA).
fn decode(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let mut rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    for px in rgba.pixels_mut() {
        px.0.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([image::Frame::new(rgba)])))
}

/// Where an image to add comes from.
pub enum Source {
    /// Pasted: the clipboard's bytes.
    Bytes(Vec<u8>),
    /// A file, dropped or chosen.
    File(PathBuf),
    /// Downloaded from `url` (an image of the note's that lived on the web).
    Downloaded { url: String, bytes: Vec<u8> },
}

impl Source {
    fn name(&self) -> String {
        match self {
            Source::Bytes(_) => tr!("The pasted image"),
            Source::Downloaded { url, .. } => url.clone(),
            Source::File(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string()),
        }
    }

    fn read(self) -> std::io::Result<Vec<u8>> {
        match self {
            Source::Bytes(b) | Source::Downloaded { bytes: b, .. } => Ok(b),
            Source::File(p) => {
                use std::io::Read;
                let mut out = vec![];
                std::fs::File::open(p)?
                    .take(READ_AT_MOST + 1)
                    .read_to_end(&mut out)?;
                Ok(out)
            }
        }
    }
}

/// Prepares and keeps each of `sources` for a note whose text is `text`
/// (off the UI thread): the ids kept, each with its source's index, and
/// what went wrong with the rest.
pub fn add(vault: &Vault, text: &str, sources: Vec<Source>) -> (Vec<(usize, String)>, Vec<String>) {
    let (mut kept, mut problems) = (vec![], vec![]);
    let mut total = vault.attachments_size(&attachment_refs(text)).unwrap_or(0);
    for (index, source) in sources.into_iter().enumerate() {
        let name = source.name();
        let bytes = match source.read() {
            Ok(b) if b.len() as u64 > READ_AT_MOST => {
                problems.push(tr!("{name} is too large to be an image here.", name = name));
                continue;
            }
            Ok(b) => b,
            Err(e) => {
                problems.push(tr!(
                    "{name} cannot be read: {e}",
                    name = name,
                    e = e.to_string()
                ));
                continue;
            }
        };
        let prepared = match prepare(&bytes) {
            Ok(p) => p,
            Err(e) => {
                problems.push(said_prepare(&name, &e));
                continue;
            }
        };
        if total + prepared.bytes.len() > limits::NOTE_ATTACHMENTS {
            problems.push(crate::environment::said_oversize(
                &Oversize::NoteAttachments(total + prepared.bytes.len()),
            ));
            break;
        }
        match vault.add_attachment(&prepared.bytes) {
            Ok(id) => {
                total += prepared.bytes.len();
                kept.push((index, id));
            }
            Err(e) => problems.push(crate::environment::said_error(&e)),
        }
    }
    (kept, problems)
}

fn said_prepare(name: &str, e: &PrepareError) -> String {
    match e {
        PrepareError::Unreadable(_) => tr!("{name} is not an image that can be read.", name = name),
        PrepareError::TooManyPixels { width, height } => tr!(
            "{name} is too large an image ({width} × {height}).",
            name = name,
            width = *width,
            height = *height
        ),
        PrepareError::TooLarge(n) => crate::environment::said_oversize(&Oversize::Attachment(*n)),
    }
}

/// The image an attachment holds, with its extension (`png` or `jpg`).
pub fn image_bytes(vault: &Vault, id: &str) -> Option<(Vec<u8>, &'static str)> {
    let bytes = vault.attachment(id).ok()??.to_vec();
    let ext = match shellwing_vault::image_mime(&bytes)? {
        "image/png" => "png",
        _ => "jpg",
    };
    Some((bytes, ext))
}

/// Where dropped images go: the start of the line under the pointer
/// (`offset`) if it is at one, else the end of that line, so a line of text
/// is never split.
pub fn drop_offset(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if offset == 0 || text[..offset].ends_with('\n') {
        return offset;
    }
    text[offset..].find('\n').map_or(text.len(), |i| offset + i)
}

/// The images written in `text` as `data:` URIs (pasted Markdown from
/// elsewhere): each URI (as it is in the text) and its bytes.
pub fn data_images(text: &str) -> Vec<(String, Vec<u8>)> {
    use base64::Engine as _;
    let mut out = vec![];
    let mut rest = text;
    while let Some(at) = rest.find("](data:image/") {
        let uri_start = at + 2;
        let tail = &rest[uri_start..];
        let Some(end) = tail.find(')') else { break };
        let uri = &tail[..end];
        if let Some((_, data)) = uri.split_once(";base64,")
            && let Ok(bytes) = base64::engine::general_purpose::STANDARD
                .decode(data.trim().replace(['\n', '\r', ' '], ""))
        {
            out.push((uri.to_string(), bytes));
        }
        rest = &tail[end..];
    }
    out
}

/// Writes the note `text` titled `title`, with its images, into a folder of
/// its own in `dir`: `note.md`, its images in `images/`, the references
/// rewritten to them. The folder, and how many images were not here to
/// write.
pub fn export(
    vault: &Vault,
    title: &str,
    text: &str,
    dir: &std::path::Path,
) -> std::io::Result<(PathBuf, usize)> {
    let name: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_.".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();
    let name = if name.is_empty() {
        "note".to_string()
    } else {
        name
    };
    let mut folder = dir.join(&name);
    let mut n = 2;
    while folder.exists() {
        folder = dir.join(format!("{name} ({n})"));
        n += 1;
    }
    std::fs::create_dir_all(&folder)?;
    let mut out = text.to_string();
    let mut missing = 0;
    let refs = attachment_refs(text);
    if !refs.is_empty() {
        std::fs::create_dir_all(folder.join("images"))?;
    }
    for id in refs {
        match image_bytes(vault, &id) {
            Some((bytes, ext)) => {
                let file = format!("images/{id}.{ext}");
                std::fs::write(folder.join(&file), bytes)?;
                out = out.replace(&format!("{SCHEME}{id}"), &file);
            }
            None => missing += 1,
        }
    }
    std::fs::write(folder.join("note.md"), out)?;
    Ok((folder, missing))
}

/// A web image downloads this much at most.
const DOWNLOAD_AT_MOST: u64 = 20 * 1024 * 1024;

/// The images in `text` that live on the web, `![…](http…)`: their URLs,
/// once each, in order.
pub fn remote_images(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut rest = text;
    while let Some(at) = rest.find("![") {
        rest = &rest[at + 2..];
        let Some(open) = rest.find("](") else { break };
        let tail = &rest[open + 2..];
        let Some(close) = tail.find(')') else { break };
        let url = tail[..close].trim();
        if (url.starts_with("https://") || url.starts_with("http://"))
            && !out.iter().any(|u| u == url)
        {
            out.push(url.to_string());
        }
        rest = &tail[close..];
    }
    out
}

/// The host a web image comes from.
pub fn host_of(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()?
        .host_str()
        .map(|h| h.trim_matches(['[', ']']).to_string())
}

/// Whether `ip` is on the internet: not this computer, a private or
/// link-local network, or carrier-grade NAT. A note (perhaps written by
/// someone else) must not make the app reach into the user's own network.
pub fn is_public(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || a == 0
                || (a == 100 && (64..128).contains(&b)))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || (first & 0xfe00) == 0xfc00 // unique local
                || (first & 0xffc0) == 0xfe80) // link-local
        }
    }
}

/// Why a web image did not come.
#[derive(Debug, PartialEq, Eq)]
pub enum FetchError {
    /// Its host is on a private network (or this computer).
    Private(String),
    /// Larger than a note's image could be.
    TooLarge,
    Failed(String),
}

impl FetchError {
    pub fn said(&self, url: &str) -> String {
        match self {
            FetchError::Private(host) => tr!(
                "{host} is on a private network: its image was not downloaded.",
                host = host.as_str()
            ),
            FetchError::TooLarge => tr!("{name} is too large to be an image here.", name = url),
            FetchError::Failed(why) => tr!(
                "{name} could not be downloaded: {e}",
                name = url,
                e = why.as_str()
            ),
        }
    }
}

/// For the UI tests: their web server is on this computer.
#[cfg(all(test, feature = "ui-tests"))]
pub static ALLOW_LOCAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The public addresses `url`'s host has, or why it may not be reached.
fn public_addrs(url: &reqwest::Url) -> Result<Vec<std::net::SocketAddr>, FetchError> {
    use std::net::ToSocketAddrs;
    let host = url
        .host_str()
        .ok_or_else(|| FetchError::Failed("no host".into()))?
        .trim_matches(['[', ']'])
        .to_string();
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs: Vec<_> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| FetchError::Failed(e.to_string()))?
        .collect();
    #[cfg(all(test, feature = "ui-tests"))]
    if ALLOW_LOCAL.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(addrs);
    }
    if addrs.is_empty() || addrs.iter().any(|a| !is_public(a.ip())) {
        return Err(FetchError::Private(host));
    }
    Ok(addrs)
}

/// Downloads a note's web image (off the UI thread): its host must be on
/// the internet (checked again at each redirect), and the connection goes to
/// the addresses checked, so a name that changes its answer cannot lead it
/// elsewhere. 20 MB at most, 30 s in all.
pub fn fetch(url: &str) -> Result<Vec<u8>, FetchError> {
    let failed = |e: &dyn std::fmt::Display| FetchError::Failed(e.to_string());
    let parsed = reqwest::Url::parse(url).map_err(|e| failed(&e))?;
    if !matches!(parsed.scheme(), "https" | "http") {
        return Err(FetchError::Failed(parsed.scheme().to_string()));
    }
    let addrs = public_addrs(&parsed)?;
    let host = parsed.host_str().unwrap_or_default().to_string();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let redirects = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            attempt.error("too many redirects")
        } else if public_addrs(attempt.url()).is_err() {
            attempt.error("a redirect to a private network")
        } else {
            attempt.follow()
        }
    });
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .redirect(redirects)
        .resolve_to_addrs(&host, &addrs)
        .user_agent(concat!("shellwing/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| failed(&e))?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| failed(&e))?;
    rt.block_on(async move {
        let mut res = client.get(url).send().await.map_err(|e| failed(&e))?;
        if !res.status().is_success() {
            return Err(FetchError::Failed(res.status().to_string()));
        }
        if res.content_length().is_some_and(|n| n > DOWNLOAD_AT_MOST) {
            return Err(FetchError::TooLarge);
        }
        let mut out = vec![];
        while let Some(chunk) = res.chunk().await.map_err(|e| failed(&e))? {
            out.extend_from_slice(&chunk);
            if out.len() as u64 > DOWNLOAD_AT_MOST {
                return Err(FetchError::TooLarge);
            }
        }
        Ok(out)
    })
}

/// What to put in place of `range` of `text` for images `ids`: one line each,
/// on lines of their own (a line break is added before or after when the
/// text around would share their lines).
pub fn insertion(text: &str, range: Range<usize>, ids: &[String]) -> String {
    let lines: Vec<String> = ids.iter().map(|id| attachment_line(id)).collect();
    let mut out = String::new();
    let before = text.get(..range.start).unwrap_or_default();
    if !before.is_empty() && !before.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&lines.join("\n"));
    let after = text.get(range.end..).unwrap_or_default();
    if !after.is_empty() && !after.starts_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_go_on_lines_of_their_own() {
        let ids = ["0123456789abcdef0123456789abcdef".to_string()];
        let line = attachment_line(&ids[0]);
        // Mid-line: broken before and after.
        let text = "before after";
        assert_eq!(insertion(text, 7..7, &ids), format!("\n{line}\n"));
        // At a line's start and end: nothing added.
        assert_eq!(insertion("a\n\nb", 2..2, &ids), line);
        assert_eq!(insertion("", 0..0, &ids), line);
        // At the end of a line with text after on the next: only before.
        assert_eq!(insertion("a\nb", 1..1, &ids), format!("\n{line}"));
        // Two at once, one line each.
        let two = [
            ids[0].clone(),
            "fedcba9876543210fedcba9876543210".to_string(),
        ];
        assert_eq!(
            insertion("", 0..0, &two),
            format!("{line}\n{}", attachment_line(&two[1]))
        );
    }

    #[test]
    fn dropped_images_do_not_split_a_line() {
        let text = "first line\nsecond line\n";
        // At a line's start: there.
        assert_eq!(drop_offset(text, 11), 11);
        assert_eq!(drop_offset(text, 0), 0);
        // In a line: at its end.
        assert_eq!(drop_offset(text, 4), 10);
        assert_eq!(drop_offset("no newline", 3), 10);
    }

    #[test]
    fn data_uris_are_found_and_decoded() {
        let text = "a ![x](data:image/png;base64,iVBORw0K) b ![y](https://e.com/y.png) \
                    ![z](data:image/jpeg;base64,/9j/4A==)";
        let found = data_images(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, "data:image/png;base64,iVBORw0K");
        assert!(found[0].1.starts_with(b"\x89PNG"));
        assert_eq!(found[1].1, [0xFF, 0xD8, 0xFF, 0xE0]);
    }

    #[test]
    fn web_images_are_found_once_each() {
        let text = "![a](https://e.com/a.png)\n![b](http://x.org/b.jpg){width=300}\n\
                    ![c](attachment:0123456789abcdef0123456789abcdef)\n![a](https://e.com/a.png)\n\
                    [a link](https://e.com/page)";
        assert_eq!(
            remote_images(text),
            ["https://e.com/a.png", "http://x.org/b.jpg"]
        );
        assert_eq!(
            host_of("https://i.imgur.com/x.png").as_deref(),
            Some("i.imgur.com")
        );
        assert_eq!(host_of("http://[::1]:8080/x.png").as_deref(), Some("::1"));
    }

    #[test]
    fn private_networks_are_not_reached() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.9",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.1",
        ] {
            assert!(!is_public(ip.parse().expect("ip")), "{ip} is private");
        }
        for ip in ["1.1.1.1", "140.82.112.3", "2606:4700:4700::1111"] {
            assert!(is_public(ip.parse().expect("ip")), "{ip} is public");
        }
        assert_eq!(
            fetch("http://127.0.0.1:9/x.png"),
            Err(FetchError::Private("127.0.0.1".into()))
        );
        assert_eq!(
            fetch("http://localhost:9/x.png"),
            Err(FetchError::Private("localhost".into()))
        );
    }

    #[test]
    fn sources_name_attachments_only() {
        assert_eq!(
            attachment_id("attachment:0123456789abcdef0123456789abcdef"),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(attachment_id("https://example.com/a.png"), None);
        assert_eq!(attachment_id("attachment:../../etc/passwd"), None);
    }

    #[test]
    fn decoded_for_gpui_in_bgra() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 1, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("png");
        let image = decode(png.get_ref()).expect("decode");
        let frame = image.as_bytes(0).expect("frame");
        assert_eq!(&frame[..4], &[30, 20, 10, 255]);
    }
}
