//! How this copy of the app was installed, as far as can be told from where
//! it runs: a copy the app may replace itself, or one something else
//! updates (a package manager, a store), which a replacement would leave
//! thinking it has the old one.

use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// Downloaded and put there by the user (an AppImage, an .app from a
    /// disk image, a folder on Windows).
    Direct,
    /// Updated by something else: what, and the command that does it, if one.
    Managed {
        by: &'static str,
        command: Option<&'static str>,
    },
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
    if var("APPIMAGE").is_some() {
        return Install::Direct;
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
    if p.contains(".app/contents/macos/")
        && exe
            .ancestors()
            .nth(2)
            .is_some_and(|contents| contents.join("_MASReceipt").exists())
    {
        return managed("the App Store", None);
    }
    // Under /usr (not /usr/local): a package of the system's.
    if p.starts_with("/usr/") && !p.starts_with("/usr/local/") {
        return managed("the system's package manager", None);
    }
    Install::Direct
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
        assert_eq!(install("/home/me/Apps/shellwing", &[]), Install::Direct);
        assert_eq!(
            install(
                "/tmp/.mount_x/usr/bin/shellwing",
                &[("APPIMAGE", "/home/me/s.AppImage")]
            ),
            Install::Direct
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
        assert_eq!(install("/usr/local/bin/shellwing", &[]), Install::Direct);
    }
}
