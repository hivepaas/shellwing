//! How this copy of the app was installed, as far as can be told from where
//! it runs: a copy the app can replace itself (an AppImage, an `.app`, one put
//! there by its installer on Windows), one put somewhere by hand that it can
//! only point to the download of, or one something else updates (a package
//! manager, a store), which a replacement would leave thinking it has the
//! old one.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// An AppImage, at this path: replaced by the release's.
    AppImage(PathBuf),
    /// An app bundle on macOS, this `.app`: replaced whole.
    MacBundle(PathBuf),
    /// Put there by shellwing's installer on Windows: the next one installs
    /// over it.
    WindowsInstaller,
    /// Put there by hand (a binary run from a folder): the app says where
    /// the release is, and the user puts it there.
    Manual,
    /// Updated by something else: what, and the command that does it, if one.
    Managed {
        by: &'static str,
        command: Option<&'static str>,
    },
}

impl Install {
    /// The app can download a release and install it itself.
    pub fn updates_itself(&self) -> bool {
        matches!(
            self,
            Install::AppImage(_) | Install::MacBundle(_) | Install::WindowsInstaller
        )
    }
}

/// The `.app` an executable inside one belongs to (`X.app/Contents/MacOS/x`).
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let bundle = exe.parent()?.parent()?.parent()?;
    let macos = exe.parent()?.file_name()? == "MacOS";
    let contents = exe.parent()?.parent()?.file_name()? == "Contents";
    let app = bundle.extension().is_some_and(|e| e == "app");
    (macos && contents && app).then(|| bundle.to_path_buf())
}

/// How the app at `exe` was installed; `var` reads the environment.
pub fn install_of(exe: &Path, var: impl Fn(&str) -> Option<String>) -> Install {
    let managed = |by, command| Install::Managed { by, command };
    if var("FLATPAK_ID").is_some() {
        return managed("Flatpak", Some("flatpak update"));
    }
    if var("SNAP").is_some() {
        return managed("Snap", Some("sudo snap refresh shellwing"));
    }
    // An AppImage runs from a mount of itself; it names its file.
    if let Some(image) = var("APPIMAGE").filter(|p| !p.is_empty()) {
        return Install::AppImage(PathBuf::from(image));
    }
    let p = exe.to_string_lossy().replace('\\', "/").to_lowercase();
    if p.contains("/cellar/") || p.contains("/homebrew/") || p.contains("/linuxbrew/") {
        return managed("Homebrew", Some("brew upgrade shellwing"));
    }
    if p.contains("/scoop/apps/") {
        return managed("Scoop", Some("scoop update shellwing"));
    }
    if p.contains("/windowsapps/") {
        return managed("the Microsoft Store", None);
    }
    if let Some(bundle) = bundle_of(exe) {
        if bundle.join("Contents/_MASReceipt").exists() {
            return managed("the App Store", None);
        }
        return Install::MacBundle(bundle);
    }
    // Where shellwing's installer puts it: for the user, or for everyone.
    if p.ends_with(".exe")
        && (p.contains("/appdata/local/programs/shellwing/")
            || p.contains("/program files/shellwing/"))
    {
        return Install::WindowsInstaller;
    }
    // Under /usr (not /usr/local): a package of the system's.
    if p.starts_with("/usr/") && !p.starts_with("/usr/local/") {
        return managed("the system's package manager", None);
    }
    Install::Manual
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(path: &str, vars: &[(&str, &str)]) -> Install {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        install_of(&PathBuf::from(path), move |k| {
            vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
        })
    }

    #[test]
    fn what_updates_the_app_is_told_by_where_it_runs() {
        assert_eq!(install("/home/me/bin/shellwing", &[]), Install::Manual);
        assert_eq!(
            install(
                "/tmp/.mount_x/usr/bin/shellwing",
                &[("APPIMAGE", "/home/me/Apps/shellwing.AppImage")]
            ),
            Install::AppImage("/home/me/Apps/shellwing.AppImage".into())
        );
        assert_eq!(
            install("/Applications/Shellwing.app/Contents/MacOS/shellwing", &[]),
            Install::MacBundle("/Applications/Shellwing.app".into())
        );
        assert_eq!(
            install(
                r"C:\Users\me\AppData\Local\Programs\shellwing\shellwing.exe",
                &[]
            ),
            Install::WindowsInstaller
        );
        assert!(matches!(
            install("/app/bin/shellwing", &[("FLATPAK_ID", "dev.shellwing")]),
            Install::Managed { by: "Flatpak", .. }
        ));
        assert!(matches!(
            install("/opt/homebrew/Cellar/shellwing/0.2.0/bin/shellwing", &[]),
            Install::Managed {
                by: "Homebrew",
                command: Some("brew upgrade shellwing")
            }
        ));
        assert!(matches!(
            install(
                r"C:\Users\me\scoop\apps\shellwing\current\shellwing.exe",
                &[]
            ),
            Install::Managed { by: "Scoop", .. }
        ));
        assert!(matches!(
            install("/usr/bin/shellwing", &[]),
            Install::Managed {
                by: "the system's package manager",
                ..
            }
        ));
        assert_eq!(install("/usr/local/bin/shellwing", &[]), Install::Manual);
        assert!(Install::AppImage("/x".into()).updates_itself());
        assert!(!Install::Manual.updates_itself());
    }
}
