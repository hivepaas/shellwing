//! The icons a host's tile can show: systems' marks, in their own colors,
//! and themes', in the tile's. A host names one with `icon` in hosts.toml,
//! or leaves it to the system it was seen running.

use shellwing_i18n::tr;

#[derive(Debug, PartialEq, Eq)]
pub struct HostIcon {
    /// What hosts.toml names it by.
    pub id: &'static str,
    /// A system's own name; a theme's is said in the app's language
    /// ([`label`]).
    pub name: &'static str,
    pub path: &'static str,
    /// A system's mark, filled; else a theme's, drawn in strokes.
    pub system: bool,
    /// A system's own color, where it reads on the tile; None: the tile's.
    pub color: Option<u32>,
}

macro_rules! system {
    ($id:literal, $name:literal, $color:expr) => {
        HostIcon {
            id: $id,
            name: $name,
            path: concat!("icons/os/", $id, ".svg"),
            system: true,
            color: $color,
        }
    };
}

macro_rules! theme {
    ($id:literal, $path:literal) => {
        HostIcon {
            id: $id,
            name: "",
            path: $path,
            system: false,
            color: None,
        }
    };
}

/// Systems' marks, from Simple Icons (assets/icons/os/README.md). A black
/// mark, or one with no color of its own that reads (Linux's yellow), takes
/// the tile's.
pub const SYSTEMS: &[HostIcon] = &[
    system!("ubuntu", "Ubuntu", Some(0xe95420)),
    system!("debian", "Debian", Some(0xa81d33)),
    system!("fedora", "Fedora", Some(0x51a2da)),
    system!("redhat", "Red Hat", Some(0xee0000)),
    system!("centos", "CentOS", Some(0x262577)),
    system!("rocky", "Rocky Linux", Some(0x10b981)),
    system!("alma", "AlmaLinux", None),
    system!("alpine", "Alpine Linux", Some(0x0d597f)),
    system!("arch", "Arch Linux", Some(0x1793d1)),
    system!("opensuse", "openSUSE", Some(0x73ba25)),
    system!("kali", "Kali Linux", Some(0x557c94)),
    system!("mint", "Linux Mint", Some(0x86be43)),
    system!("raspberry-pi", "Raspberry Pi", Some(0xa22846)),
    system!("freebsd", "FreeBSD", Some(0xab2b28)),
    system!("macos", "macOS", None),
    system!("nixos", "NixOS", Some(0x5277c3)),
    system!("proxmox", "Proxmox", Some(0xe57000)),
    system!("truenas", "TrueNAS", Some(0x0095d5)),
    system!("openwrt", "OpenWrt", Some(0x00b5e2)),
    system!("linux", "Linux", None),
];

pub const THEMES: &[HostIcon] = &[
    theme!("server", "icons/server.svg"),
    theme!("database", "icons/theme/database.svg"),
    theme!("web", "icons/theme/web.svg"),
    theme!("cloud", "icons/cloud.svg"),
    theme!("container", "icons/theme/container.svg"),
    theme!("firewall", "icons/theme/firewall.svg"),
    theme!("router", "icons/theme/router.svg"),
    theme!("storage", "icons/theme/storage.svg"),
    theme!("home", "icons/home.svg"),
    theme!("terminal", "icons/terminal.svg"),
    theme!("git", "icons/theme/git.svg"),
    theme!("compute", "icons/theme/compute.svg"),
    theme!("mail", "icons/theme/mail.svg"),
    theme!("desktop", "icons/laptop.svg"),
    theme!("bastion", "icons/theme/bastion.svg"),
    theme!("test", "icons/theme/test.svg"),
];

/// The icon named `id`, if the app has it.
pub fn find(id: &str) -> Option<&'static HostIcon> {
    SYSTEMS.iter().chain(THEMES).find(|i| i.id == id)
}

/// What the icon is called, in the app's language for a theme's.
pub fn label(icon: &HostIcon) -> String {
    match icon.id {
        "server" => tr!("Server"),
        "database" => tr!("Database"),
        "web" => tr!("Web"),
        "cloud" => tr!("Cloud"),
        "container" => tr!("Container"),
        "firewall" => tr!("Firewall"),
        "router" => tr!("Router"),
        "storage" => tr!("Storage"),
        "home" => tr!("Home network"),
        "terminal" => tr!("Terminal"),
        "git" => tr!("Git"),
        "compute" => tr!("Compute"),
        "mail" => tr!("Mail"),
        "desktop" => tr!("Desktop"),
        "bastion" => tr!("Bastion"),
        "test" => tr!("Test"),
        _ => icon.name.to_string(),
    }
}

/// Whether `color` stands out from `ground` enough to draw a mark in: their
/// contrast, by WCAG's relative luminance, at least 1.6 to 1. A mark is a
/// shape, not text, and its color is much of what makes it known.
pub fn reads_on(color: u32, ground: u32) -> bool {
    fn lum(c: u32) -> f32 {
        let ch = |s: u32| {
            let v = ((c >> s) & 0xff) as f32 / 255.;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
    }
    let (a, b) = (lum(color), lum(ground));
    (a.max(b) + 0.05) / (a.min(b) + 0.05) >= 1.6
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon a host can name, or be seen running, is drawn into the app.
    #[test]
    fn every_icon_is_in_the_binary() {
        for icon in SYSTEMS.iter().chain(THEMES) {
            assert!(
                crate::ICONS.iter().any(|(p, _)| *p == icon.path),
                "{} is not among the app's icons",
                icon.path
            );
        }
        for id in [
            "ubuntu",
            "debian",
            "fedora",
            "redhat",
            "centos",
            "rocky",
            "alma",
            "alpine",
            "arch",
            "opensuse",
            "kali",
            "mint",
            "raspberry-pi",
            "freebsd",
            "macos",
            "nixos",
            "proxmox",
            "openwrt",
            "linux",
        ] {
            assert!(find(id).is_some_and(|i| i.system), "{id}");
        }
    }

    #[test]
    fn a_dark_mark_on_a_dark_tile_does_not_read() {
        let (light, dark) = (0xdce2e9, 0x2f3641);
        assert!(reads_on(0xe95420, light));
        assert!(reads_on(0xe95420, dark));
        assert!(!reads_on(0x262577, dark));
    }
}
