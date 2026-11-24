//! Reading a file only an administrator can, such as a key in `/root/.ssh`:
//! once, with the system's own password prompt (polkit's `pkexec` on Linux,
//! `osascript`'s administrator privileges on macOS). What it reads is handed
//! back, never written anywhere; the caller keeps it in the vault.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use zeroize::Zeroizing;

/// More than any key file: a file this big is not one, and is not read.
pub const MOST: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The prompt was dismissed.
    Cancelled,
    /// The password was not taken, or the account may not do this.
    NotAllowed,
    /// This system has no way to ask (no `pkexec`), or it is not one this
    /// is made for.
    Unsupported,
    /// Asked and allowed, but the file was not read: why, as the system said.
    Failed(String),
}

/// Whether this system can ask for an administrator's password to read a
/// file.
pub fn supported() -> bool {
    program().is_some()
}

/// Reads `path` as an administrator, asking for the password. Blocks until
/// it is answered: run it off the UI thread.
pub fn read(path: &Path) -> Result<Zeroizing<String>, Error> {
    let (program, args) = program().ok_or(Error::Unsupported)?;
    let cat = find("cat").ok_or(Error::Unsupported)?;
    let out = Command::new(program)
        .args(args(&cat, path))
        .output()
        .map_err(|e| Error::Failed(e.to_string()))?;
    said(out)
}

type Args = fn(&Path, &Path) -> Vec<std::ffi::OsString>;

/// The asking program, and its arguments for `cat` of a path.
fn program() -> Option<(PathBuf, Args)> {
    if cfg!(target_os = "macos") {
        let osascript = PathBuf::from("/usr/bin/osascript");
        osascript
            .exists()
            .then_some((osascript, macos_args as Args))
    } else if cfg!(any(target_os = "linux", target_os = "freebsd")) {
        find("pkexec").map(|p| (p, pkexec_args as Args))
    } else {
        None
    }
}

/// `pkexec` runs the program as root after polkit's prompt; it wants the
/// program's full path, as `PATH` is not root's.
fn pkexec_args(cat: &Path, path: &Path) -> Vec<std::ffi::OsString> {
    vec![cat.into(), "--".into(), path.into()]
}

/// The path is the script's argument, never part of its text, so no name
/// can be read as AppleScript or shell; `quoted form of` quotes it for the
/// shell `do shell script` runs.
fn macos_args(cat: &Path, path: &Path) -> Vec<std::ffi::OsString> {
    let script = format!(
        "on run argv\n\
         do shell script \"{} -- \" & quoted form of (item 1 of argv) with administrator privileges\n\
         end run",
        cat.display()
    );
    vec!["-e".into(), script.into(), path.into()]
}

/// What the asking program's end says.
fn said(out: Output) -> Result<Zeroizing<String>, Error> {
    let stdout = Zeroizing::new(out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if out.status.success() {
        if stdout.len() > MOST {
            return Err(Error::Failed(format!("larger than {} KB", MOST / 1024)));
        }
        return String::from_utf8(stdout.to_vec())
            .map(Zeroizing::new)
            .map_err(|_| Error::Failed("not text".into()));
    }
    // pkexec: 126 for a dismissed prompt or no authorization, 127 when it
    // could not ask at all. osascript: error -128 is "User canceled".
    match out.status.code() {
        Some(126) if stderr.is_empty() || stderr.contains("dismissed") => Err(Error::Cancelled),
        Some(126) | Some(127) => Err(Error::NotAllowed),
        _ if stderr.contains("(-128)") => Err(Error::Cancelled),
        _ if stderr.contains("(-60005)") || stderr.contains("(-60007)") => Err(Error::NotAllowed),
        _ => Err(Error::Failed(last_words(&stderr))),
    }
}

/// The part of an error that says what went wrong: `cat`'s own words after
/// its name and the path (`cat: /root/x: No such file or directory`).
fn last_words(stderr: &str) -> String {
    let line = stderr.lines().last().unwrap_or("").trim();
    line.rsplit(": ")
        .next()
        .unwrap_or(line)
        .trim_end_matches('.')
        .to_string()
}

/// `name` in the usual system folders, then in `PATH`.
fn find(name: &str) -> Option<PathBuf> {
    let usual = [
        "/usr/bin",
        "/bin",
        "/usr/local/bin",
        "/run/current-system/sw/bin",
    ]
    .into_iter()
    .map(PathBuf::from);
    let path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    usual
        .chain(path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn reads_what_was_printed() {
        let key = "-----BEGIN OPENSSH PRIVATE KEY-----\n…\n";
        assert_eq!(
            said(out(0, key, "")).as_deref().map(|s| s.as_str()),
            Ok(key)
        );
        let big = "x".repeat(MOST + 1);
        assert!(matches!(said(out(0, &big, "")), Err(Error::Failed(_))));
    }

    #[test]
    fn says_why_not() {
        assert_eq!(said(out(126, "", "")), Err(Error::Cancelled));
        assert_eq!(
            said(out(
                126,
                "",
                "Error executing command as another user: Request dismissed"
            )),
            Err(Error::Cancelled)
        );
        assert_eq!(
            said(out(
                126,
                "",
                "Error executing command as another user: Not authorized"
            )),
            Err(Error::NotAllowed)
        );
        assert_eq!(said(out(127, "", "")), Err(Error::NotAllowed));
        assert_eq!(
            said(out(1, "", "0:120: execution error: User canceled. (-128)")),
            Err(Error::Cancelled)
        );
        assert_eq!(
            said(out(
                1,
                "",
                "/bin/cat: /root/.ssh/nope: No such file or directory"
            )),
            Err(Error::Failed("No such file or directory".into()))
        );
    }

    #[test]
    fn the_path_is_an_argument_never_the_script() {
        let odd = Path::new("/root/.ssh/a\" & do shell script \"rm -rf ~");
        let args = macos_args(Path::new("/bin/cat"), odd);
        assert_eq!(args.last().map(|a| a.as_os_str()), Some(odd.as_os_str()));
        assert!(!args[1].to_string_lossy().contains("rm -rf"));
        let args = pkexec_args(Path::new("/usr/bin/cat"), Path::new("-n"));
        assert_eq!(
            args,
            ["/usr/bin/cat", "--", "-n"].map(std::ffi::OsString::from)
        );
    }
}
