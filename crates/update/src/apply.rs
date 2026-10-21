//! Installing a downloaded release in place of the app, as it was installed
//! (see [`Install`]):
//!
//! - **An AppImage** is replaced by the new one: written beside it, then
//!   renamed over it, so it is whole or not at all.
//! - **An `.app`** is replaced whole: the new one is unpacked beside it,
//!   checked (`codesign`: valid, and signed by the same team as the one
//!   running), and the two are swapped; the old one is put back if the swap
//!   fails half way.
//! - **On Windows**, the release's installer runs, quiet, once the app has
//!   quit.
//!
//! Then, when asked, the new app starts.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use shellwing_spawn::Unblocked;

use crate::install::Install;

/// Installs the release in `staged` over the app installed as `install`,
/// then starts it when `relaunch`. Call it as the app quits.
pub fn apply(install: &Install, staged: &Path, relaunch: bool) -> io::Result<()> {
    match install {
        Install::AppImage(image) => {
            replace_file(image, staged)?;
            if relaunch {
                // The new app must not start with signals blocked.
                Command::new(image).spawn_unblocked()?;
            }
            Ok(())
        }
        Install::MacBundle(bundle) => {
            replace_bundle(bundle, staged, signed_alike)?;
            if relaunch {
                Command::new("open")
                    .arg("-n")
                    .arg(bundle)
                    .spawn_unblocked()?;
            }
            Ok(())
        }
        Install::WindowsInstaller => run_installer(staged, relaunch),
        Install::Manual | Install::Managed { .. } => Err(io::Error::other(
            "this copy of the app is not one it can update itself",
        )),
    }
}

/// `target` replaced by a copy of `new`, executable: written beside it, then
/// renamed over it.
pub fn replace_file(target: &Path, new: &Path) -> io::Result<()> {
    let dir = target
        .parent()
        .ok_or_else(|| io::Error::other("no folder"))?;
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::other("no file name"))?
        .to_string_lossy()
        .to_string();
    let tmp = dir.join(format!(".{name}.update"));
    std::fs::copy(new, &tmp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, target).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// `bundle` (an `.app`) replaced by the one in the tar.gz `archive`, once
/// `check(new, old)` accepts it.
pub fn replace_bundle(
    bundle: &Path,
    archive: &Path,
    check: impl Fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let parent = bundle
        .parent()
        .ok_or_else(|| io::Error::other("no folder"))?;
    // Beside it, so the swap is a rename on one volume.
    let work = parent.join(format!(".shellwing-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir(&work)?;
    let done = (|| {
        let file = std::fs::File::open(archive)?;
        // Entries that would land outside `work` (`..`, absolute) are refused.
        tar::Archive::new(flate2::read::GzDecoder::new(file)).unpack(work.join("new"))?;
        let new = only_app_in(&work.join("new"))?;
        check(&new, bundle)?;
        let old = work.join("old.app");
        std::fs::rename(bundle, &old)?;
        if let Err(e) = std::fs::rename(&new, bundle) {
            // Back as it was.
            let _ = std::fs::rename(&old, bundle);
            return Err(e);
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&work);
    done
}

/// The one `.app` in `dir`.
fn only_app_in(dir: &Path) -> io::Result<PathBuf> {
    let apps: Vec<PathBuf> = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "app") && p.is_dir())
        .collect();
    match apps.as_slice() {
        [one] => Ok(one.clone()),
        _ => Err(io::Error::other("the release holds no single .app")),
    }
}

/// The new `.app` is validly signed, by the team that signed the running one
/// (unless that one is not signed: a build of one's own).
fn signed_alike(new: &Path, old: &Path) -> io::Result<()> {
    let ok = Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(new)
        .status()?
        .success();
    if !ok {
        return Err(io::Error::other("the new app's signature does not verify"));
    }
    let team = |app: &Path| -> io::Result<Option<String>> {
        let out = Command::new("codesign")
            .args(["-dv", "--verbose=2"])
            .arg(app)
            .output()?;
        // codesign says it on stderr.
        Ok(String::from_utf8_lossy(&out.stderr)
            .lines()
            .find_map(|l| l.strip_prefix("TeamIdentifier="))
            .filter(|t| *t != "not set")
            .map(str::to_string))
    };
    match team(old)? {
        None => Ok(()),
        Some(t) if team(new)?.as_ref() == Some(&t) => Ok(()),
        Some(_) => Err(io::Error::other(
            "the new app is signed by another team than this one",
        )),
    }
}

/// The installer in `setup`, run quiet once this app has gone, then the app
/// again when `relaunch`.
fn run_installer(setup: &Path, relaunch: bool) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let exe = std::env::current_exe()?;
        // cmd waits a moment for this app to quit, runs the installer, and
        // starts the app it installed.
        let mut line = format!(
            "/C timeout /t 2 /nobreak >NUL & start \"\" /wait \"{}\" /S",
            setup.display()
        );
        if relaunch {
            line.push_str(&format!(" & start \"\" \"{}\"", exe.display()));
        }
        Command::new("cmd")
            .raw_arg(line)
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
            .spawn()?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (setup, relaunch);
        Err(io::Error::other("an installer runs on Windows only"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("shellwing-apply-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn an_appimage_is_replaced_whole_and_left_runnable() {
        let dir = temp("appimage");
        let image = dir.join("shellwing.AppImage");
        let new = dir.join("downloaded");
        std::fs::write(&image, b"old").expect("old");
        std::fs::write(&new, b"new").expect("new");
        replace_file(&image, &new).expect("replace");
        assert_eq!(std::fs::read(&image).expect("read"), b"new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&image)
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "executable");
        }
        // Nothing left beside it.
        let left: Vec<_> = std::fs::read_dir(&dir).expect("ls").flatten().collect();
        assert_eq!(left.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A tar.gz holding `Shellwing.app` with one file saying `what`.
    fn app_archive(path: &Path, what: &str) {
        let file = std::fs::File::create(path).expect("archive");
        let gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        let mut head = tar::Header::new_gnu();
        head.set_size(what.len() as u64);
        head.set_mode(0o755);
        head.set_cksum();
        tar.append_data(
            &mut head,
            "Shellwing.app/Contents/MacOS/shellwing",
            what.as_bytes(),
        )
        .expect("append");
        tar.into_inner().expect("tar").finish().expect("gz");
    }

    #[test]
    fn an_app_bundle_is_swapped_once_checked_and_kept_when_refused() {
        let dir = temp("bundle");
        let bundle = dir.join("Shellwing.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("app");
        std::fs::write(bundle.join("Contents/MacOS/shellwing"), "old").expect("old");
        let archive = dir.join("release.app.tar.gz");
        app_archive(&archive, "new");
        // Refused: the running one stays as it was.
        let refused = replace_bundle(&bundle, &archive, |_, _| Err(io::Error::other("no")));
        assert!(refused.is_err());
        assert_eq!(
            std::fs::read_to_string(bundle.join("Contents/MacOS/shellwing")).expect("read"),
            "old"
        );
        // Accepted: swapped, and nothing left beside it.
        replace_bundle(&bundle, &archive, |new, old| {
            assert!(new.ends_with("Shellwing.app") && old == bundle);
            Ok(())
        })
        .expect("swap");
        assert_eq!(
            std::fs::read_to_string(bundle.join("Contents/MacOS/shellwing")).expect("read"),
            "new"
        );
        let left: Vec<String> = std::fs::read_dir(&dir)
            .expect("ls")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(left.len(), 2, "{left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
