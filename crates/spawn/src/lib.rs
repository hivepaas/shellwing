//! Starting processes with no signals blocked.
//!
//! On macOS a child starts with the signal mask of the thread that starts
//! it: `std::process::Command` passes it on (on Linux it starts children
//! with none blocked). GPUI's background threads there, GCD workers, block
//! SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGALRM and SIGUSR1/2: a process
//! started there, and all it starts, never sees them.
//! git, failing a fetch, tells its ssh to stop with SIGTERM and waits for it
//! for ever; an ssh-agent or an editor started so cannot be stopped.

use std::io;
use std::process::{Child, Command, ExitStatus, Output};

/// Runs `start`, which starts processes, on a thread of its own with no
/// signals blocked, so they start with none blocked; what it returns.
pub fn unblocked<T: Send>(start: impl FnOnce() -> T + Send) -> T {
    #[cfg(unix)]
    {
        use nix::sys::signal::{SigSet, SigmaskHow, pthread_sigmask};
        std::thread::scope(|s| {
            s.spawn(|| {
                // This thread only: the caller's, a GCD worker's say, is left as it is.
                let _ = pthread_sigmask(SigmaskHow::SIG_SETMASK, Some(&SigSet::empty()), None);
                start()
            })
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    }
    #[cfg(not(unix))]
    start()
}

/// [`Command`]'s ways to start a process, through [`unblocked`].
pub trait Unblocked {
    fn spawn_unblocked(&mut self) -> io::Result<Child>;
    fn output_unblocked(&mut self) -> io::Result<Output>;
    fn status_unblocked(&mut self) -> io::Result<ExitStatus>;
}

impl Unblocked for Command {
    fn spawn_unblocked(&mut self) -> io::Result<Child> {
        unblocked(|| self.spawn())
    }

    fn output_unblocked(&mut self) -> io::Result<Output> {
        unblocked(|| self.output())
    }

    fn status_unblocked(&mut self) -> io::Result<ExitStatus> {
        unblocked(|| self.status())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use nix::sys::signal::{SigSet, SigmaskHow, Signal, pthread_sigmask};

    /// A shell that sends itself SIGTERM: it ends there, unless SIGTERM is
    /// blocked, when it goes on to say so.
    fn shell() -> Command {
        let mut c = Command::new("sh");
        c.args(["-c", "kill -TERM $$; sleep 1; echo survived"]);
        c
    }

    /// From a thread that blocks SIGTERM, as GPUI's background threads do.
    fn from_blocking_thread<T: Send>(f: impl FnOnce() -> T + Send) -> T {
        std::thread::scope(|s| {
            s.spawn(|| {
                let mut term = SigSet::empty();
                term.add(Signal::SIGTERM);
                pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&term), None).expect("block");
                f()
            })
            .join()
            .expect("thread")
        })
    }

    #[test]
    fn a_child_started_where_sigterm_is_blocked_still_gets_it() {
        // Where the mask is passed on (macOS), the plain start shows why.
        if cfg!(target_os = "macos") {
            let plain = from_blocking_thread(|| shell().output()).expect("sh");
            assert!(
                String::from_utf8_lossy(&plain.stdout).contains("survived"),
                "the plain start was expected to pass the mask on"
            );
        }
        let out = from_blocking_thread(|| shell().output_unblocked()).expect("sh");
        assert!(
            out.stdout.is_empty(),
            "{:?}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(!out.status.success());
    }

    #[test]
    fn the_callers_mask_is_left_as_it_was() {
        from_blocking_thread(|| {
            shell().status_unblocked().expect("sh");
            let mut now = SigSet::empty();
            pthread_sigmask(SigmaskHow::SIG_BLOCK, None, Some(&mut now)).expect("mask");
            assert!(now.contains(Signal::SIGTERM));
        });
    }
}
