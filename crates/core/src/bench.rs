//! Measurement modes, kept so performance can be re-checked after changes.
//!
//! `SHELLWING_BENCH=latency`: type "x" at the prompt 70 times and report the time
//! until the frame showing its echo is painted. `flood`: run `yes` for 4 s and
//! report frames per second. Both print a `REPORT` line and exit.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::Writer;

pub struct Probe {
    start: Instant,
    pub frames: AtomicU64,
    echo_sent: AtomicI64,
    echo_seen: AtomicBool,
    samples: Mutex<Vec<f64>>,
}

impl Default for Probe {
    fn default() -> Self {
        Self::new()
    }
}

impl Probe {
    pub fn new() -> Self {
        Probe {
            start: Instant::now(),
            frames: AtomicU64::new(0),
            echo_sent: AtomicI64::new(0),
            echo_seen: AtomicBool::new(false),
            samples: Mutex::new(vec![]),
        }
    }

    fn now(&self) -> i64 {
        self.start.elapsed().as_nanos() as i64
    }

    /// Host output passed through; marks the echo the latency test waits for.
    pub fn saw_output(&self, bytes: &[u8]) {
        if self.echo_sent.load(Ordering::Acquire) != 0 && bytes.contains(&b'x') {
            self.echo_seen.store(true, Ordering::Release);
        }
    }

    /// Called at the end of every paint.
    pub fn painted(&self) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        if self.echo_seen.swap(false, Ordering::AcqRel) {
            let sent = self.echo_sent.swap(0, Ordering::AcqRel);
            if sent != 0
                && let Ok(mut s) = self.samples.lock()
            {
                s.push((self.now() - sent) as f64 / 1e6);
            }
        }
    }
}

fn pct(xs: &mut [f64], p: f64) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs[((p * xs.len() as f64) as usize).min(xs.len() - 1)]
}

pub async fn run(mode: String, writer: Writer, probe: std::sync::Arc<Probe>) {
    let send = |s: &str| writer.write(s.as_bytes().to_vec());
    match mode.as_str() {
        "latency" => {
            tokio::time::sleep(Duration::from_millis(2500)).await;
            for _ in 0..70 {
                probe.echo_seen.store(false, Ordering::Release);
                probe.echo_sent.store(probe.now(), Ordering::Release);
                send("x");
                let deadline = Instant::now() + Duration::from_secs(2);
                while probe.echo_sent.load(Ordering::Acquire) != 0 && Instant::now() < deadline {
                    tokio::time::sleep(Duration::from_micros(200)).await;
                }
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
            send("\x15");
            let mut all = probe.samples.lock().map(|s| s.clone()).unwrap_or_default();
            let mut t = all.split_off(10.min(all.len()));
            if t.is_empty() {
                println!("REPORT latency: no samples");
            } else {
                let n = t.len();
                println!(
                    "REPORT latency echo-to-paint p50={:.1} p95={:.1} max={:.1} ms (n={n})",
                    pct(&mut t, 0.5),
                    pct(&mut t, 0.95),
                    pct(&mut t, 1.0)
                );
            }
            std::process::exit(0);
        }
        "flood" => {
            tokio::time::sleep(Duration::from_millis(2500)).await;
            send(
                "timeout 4 yes 0123456789abcdefghijklmnopqrstuvwxyz-0123456789abcdefghijklmnopqrstuvwxyz-0123456789\r",
            );
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let mut fps = vec![];
            for _ in 0..2 {
                let a = probe.frames.load(Ordering::Relaxed);
                tokio::time::sleep(Duration::from_secs(1)).await;
                fps.push(probe.frames.load(Ordering::Relaxed) - a);
            }
            println!("REPORT flood fps={fps:?}");
            std::process::exit(0);
        }
        _ => {}
    }
}
