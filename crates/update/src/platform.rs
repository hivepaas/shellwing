//! What a release has for each platform, and where it is downloaded from.
//!
//! | platform | file |
//! |---|---|
//! | `linux_x86_64`, `linux_aarch64` | `shellwing_0.2.0_linux_x86_64.AppImage` |
//! | `macos_aarch64`, `macos_x86_64` | `shellwing_0.2.0_macos_aarch64.app.tar.gz` (`Shellwing.app`, tarred) |
//! | `windows_x86_64`, `windows_aarch64` | `shellwing_0.2.0_windows_x86_64-setup.exe` (an installer, quiet with `/S`) |

use crate::version::Version;

pub const PLATFORMS: [&str; 6] = [
    "linux_x86_64",
    "linux_aarch64",
    "macos_aarch64",
    "macos_x86_64",
    "windows_x86_64",
    "windows_aarch64",
];

/// This build's platform, if releases are made for it.
pub fn current() -> Option<&'static str> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "macos",
        "windows" => "windows",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => return None,
    };
    let name = format!("{os}_{arch}");
    PLATFORMS.into_iter().find(|p| *p == name)
}

/// The file a release has for `platform`.
pub fn archive_file(v: &Version, platform: &str) -> Option<String> {
    if !PLATFORMS.contains(&platform) {
        return None;
    }
    let plain = v.to_string();
    let plain = plain.trim_start_matches('v');
    let os = platform.split('_').next()?;
    Some(match os {
        "linux" => format!("shellwing_{plain}_{platform}.AppImage"),
        "macos" => format!("shellwing_{plain}_{platform}.app.tar.gz"),
        _ => format!("shellwing_{plain}_{platform}-setup.exe"),
    })
}

/// Where releases' files are: a release's assets on GitHub; a build that
/// tries the updater (scripts/update_try.sh) names its own, as it is built.
const DOWNLOADS: &str = match option_env!("SHELLWING_UPDATE_FILES") {
    Some(url) => url,
    None => "https://github.com/hivepaas/shellwing/releases/download",
};

/// Where a release's file is downloaded from.
pub fn download_url(v: &Version, file: &str) -> String {
    format!("{DOWNLOADS}/{v}/{file}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_platform_has_its_file() {
        let v = Version::parse("v0.3.0-beta2").expect("version");
        assert_eq!(
            archive_file(&v, "linux_x86_64").as_deref(),
            Some("shellwing_0.3.0-beta2_linux_x86_64.AppImage")
        );
        assert_eq!(
            archive_file(&v, "macos_aarch64").as_deref(),
            Some("shellwing_0.3.0-beta2_macos_aarch64.app.tar.gz")
        );
        assert_eq!(
            archive_file(&v, "windows_x86_64").as_deref(),
            Some("shellwing_0.3.0-beta2_windows_x86_64-setup.exe")
        );
        assert_eq!(archive_file(&v, "plan9_mips"), None);
        assert_eq!(
            download_url(&v, "x.AppImage"),
            "https://github.com/hivepaas/shellwing/releases/download/v0.3.0-beta2/x.AppImage"
        );
        assert!(current().is_some_and(|p| PLATFORMS.contains(&p)));
    }
}
