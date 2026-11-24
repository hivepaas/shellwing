//! The icons a host's tile can show: systems' and cloud providers' marks,
//! in their own colors, and themes', in the tile's. A host names one with
//! `icon` in hosts.toml, or leaves it to the system it was seen running; a
//! cloud provider's is only ever named, never seen.

use shellwing_i18n::tr;

#[derive(Debug, PartialEq, Eq)]
pub struct HostIcon {
    /// What hosts.toml names it by.
    pub id: &'static str,
    /// A system's or provider's own name; a theme's is said in the app's
    /// language ([`label`]).
    pub name: &'static str,
    pub face: Face,
    /// The mark's or name's own color, where it reads on the tile; None:
    /// the tile's.
    pub color: Option<u32>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Face {
    /// An owner's mark, filled: the SVG's path among the app's assets.
    Mark(&'static str),
    /// Drawn in strokes, as the app's own icons are.
    Glyph(&'static str),
    /// A name set in the tile, for a provider whose mark the app may not
    /// show.
    Name(&'static str),
}

impl HostIcon {
    /// The SVG it is drawn from, unless it is a name.
    #[cfg(test)]
    fn path(&self) -> Option<&'static str> {
        match self.face {
            Face::Mark(p) | Face::Glyph(p) => Some(p),
            Face::Name(_) => None,
        }
    }
}

macro_rules! system {
    ($id:literal, $name:literal, $color:expr) => {
        HostIcon {
            id: $id,
            name: $name,
            face: Face::Mark(concat!("icons/os/", $id, ".svg")),
            color: $color,
        }
    };
}

macro_rules! theme {
    ($id:literal, $path:literal) => {
        HostIcon {
            id: $id,
            name: "",
            face: Face::Glyph($path),
            color: None,
        }
    };
}

macro_rules! cloud {
    ($id:literal, $name:literal, $color:expr) => {
        HostIcon {
            id: $id,
            name: $name,
            face: Face::Mark(concat!("icons/cloud/", $id, ".svg")),
            color: $color,
        }
    };
    ($id:literal, $name:literal, $text:literal, $color:expr) => {
        HostIcon {
            id: $id,
            name: $name,
            face: Face::Name($text),
            color: $color,
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
    // Not Microsoft's mark, which the app may not show: a window, drawn.
    HostIcon {
        id: "windows",
        name: "Windows",
        face: Face::Glyph("icons/os/windows.svg"),
        color: None,
    },
    system!("linux", "Linux", None),
];

/// Cloud providers', for a host to name: never seen on the host. Marks from
/// Simple Icons (assets/icons/cloud/README.md); the providers whose owners
/// asked Simple Icons to drop theirs are their names, set in a darker shade
/// of their color that reads on a light tile.
pub const CLOUDS: &[HostIcon] = &[
    cloud!("aws", "Amazon Web Services", "AWS", Some(0xc25e00)),
    cloud!("azure", "Microsoft Azure", "Az", Some(0x0062ad)),
    cloud!("google-cloud", "Google Cloud", Some(0x4285f4)),
    cloud!("oracle-cloud", "Oracle Cloud", "OCI", Some(0xc74634)),
    cloud!("ibm-cloud", "IBM Cloud", "IBM", Some(0x0f62fe)),
    cloud!("digitalocean", "DigitalOcean", Some(0x0080ff)),
    cloud!("hetzner", "Hetzner", Some(0xd50c2d)),
    cloud!("vultr", "Vultr", Some(0x007bfc)),
    cloud!("akamai", "Akamai (Linode)", Some(0x0096d6)),
    cloud!("ovh", "OVHcloud", Some(0x123f6d)),
    cloud!("scaleway", "Scaleway", Some(0x4f0599)),
    cloud!("upcloud", "UpCloud", Some(0x7b00ff)),
    cloud!("alibaba-cloud", "Alibaba Cloud", Some(0xff6a00)),
    cloud!("cloudflare", "Cloudflare", Some(0xf38020)),
    cloud!("openstack", "OpenStack", Some(0xed1944)),
    cloud!("vmware", "VMware", Some(0x607078)),
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

/// A Run on hosts tab's, and the host list's Actions: not a host's.
pub const RUN: HostIcon = HostIcon {
    id: "run",
    name: "",
    face: Face::Glyph("icons/run.svg"),
    color: None,
};

/// The icon named `id`, if the app has it.
pub fn find(id: &str) -> Option<&'static HostIcon> {
    SYSTEMS
        .iter()
        .chain(CLOUDS)
        .chain(THEMES)
        .find(|i| i.id == id)
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
    contrast(color, ground) >= 1.6
}

/// Whether a name set in `color` reads on `ground`: 3 to 1, as WCAG asks of
/// large or bold text.
pub fn name_reads_on(color: u32, ground: u32) -> bool {
    contrast(color, ground) >= 3.
}

/// WCAG's contrast of two colors, from their relative luminance.
fn contrast(color: u32, ground: u32) -> f32 {
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
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon a host can name, or be seen running, is drawn into the app.
    #[test]
    fn every_icon_is_in_the_binary() {
        for icon in SYSTEMS.iter().chain(CLOUDS).chain(THEMES) {
            if let Some(path) = icon.path() {
                assert!(
                    crate::ICONS.iter().any(|(p, _)| *p == path),
                    "{path} is not among the app's icons"
                );
            }
        }
        let mut ids: Vec<_> = SYSTEMS
            .iter()
            .chain(CLOUDS)
            .chain(THEMES)
            .map(|i| i.id)
            .collect();
        ids.sort();
        ids.dedup();
        assert_eq!(
            ids.len(),
            SYSTEMS.len() + CLOUDS.len() + THEMES.len(),
            "an id twice"
        );
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
            "windows",
            "linux",
        ] {
            assert!(SYSTEMS.iter().any(|i| i.id == id), "{id}");
        }
    }

    #[test]
    fn a_dark_mark_on_a_dark_tile_does_not_read() {
        let (light, dark) = (0xdce2e9, 0x2f3641);
        assert!(reads_on(0xe95420, light));
        assert!(reads_on(0xe95420, dark));
        assert!(!reads_on(0x262577, dark));
    }

    /// A provider's name is set in its color on a light tile, and in the
    /// tile's on a dark one where that does not read.
    #[test]
    fn a_providers_name_reads_on_a_light_tile() {
        let (light, dark) = (0xdce2e9, 0x2f3641);
        for icon in CLOUDS {
            if let (Face::Name(_), Some(c)) = (&icon.face, icon.color) {
                assert!(name_reads_on(c, light), "{}", icon.id);
            }
        }
        assert!(!name_reads_on(0x0062ad, dark));
    }
}
