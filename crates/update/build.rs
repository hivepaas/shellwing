//! Builds in every release key there is: keys/<id>.pub.pem, by id; or, for
//! a build that tries the updater on a list of its own
//! (scripts/update_try.sh), the ones in `$SHELLWING_RELEASE_KEYS`.

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=SHELLWING_RELEASE_KEYS");
    let dir = match std::env::var_os("SHELLWING_RELEASE_KEYS").filter(|d| !d.is_empty()) {
        Some(d) => d.into(),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("keys"),
    };
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut keys: Vec<(String, String)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let id = name.strip_suffix(".pub.pem")?.to_string();
            Some((id, e.path().display().to_string()))
        })
        .collect();
    keys.sort();
    let mut out = String::from(
        "/// Every built-in release key's id and PEM.\npub const KEYS: &[(&str, &str)] = &[\n",
    );
    for (id, path) in keys {
        let _ = writeln!(out, "    ({id:?}, include_str!({path:?})),");
    }
    out.push_str("];\n");
    let target = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("keys.rs");
    std::fs::write(target, out).expect("keys.rs");
}
