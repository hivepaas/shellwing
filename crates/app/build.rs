//! Puts the app icon and the version into shellwing.exe. Windows shows a
//! program's icon from its resources (Explorer, the taskbar, Alt+Tab), and
//! GPUI loads the window's icon from resource 1. Built on Windows only.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    windows_resources();
}

#[cfg(windows)]
fn windows_resources() {
    const ICON: &str = "../../packaging/icons/shellwing.ico";
    println!("cargo:rerun-if-changed={ICON}");
    let mut res = winresource::WindowsResource::new();
    res.set_icon_with_id(ICON, "1")
        // Task Manager shows the description as the program's name.
        .set("FileDescription", "shellwing")
        .set("ProductName", "shellwing");
    if let Err(e) = res.compile() {
        panic!("cannot put the icon into shellwing.exe: {e}");
    }
}
