//! Hybrid logical clock stamps for fields: `<ms:13>-<counter:4 hex>-<device>`.
//!
//! Fixed widths make the string order the time order, so merging compares
//! strings. A clock never goes backwards, even if the wall clock does, and it
//! moves past every stamp it has seen from other devices, so an edit made after
//! seeing another always wins over it.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Clock {
    device: String,
    last: Mutex<(u64, u32)>,
}

fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn parse(stamp: &str) -> Option<(u64, u32)> {
    let mut parts = stamp.splitn(3, '-');
    let ms = parts.next()?.parse().ok()?;
    let counter = u32::from_str_radix(parts.next()?, 16).ok()?;
    Some((ms, counter))
}

impl Clock {
    pub fn new(device: &str) -> Self {
        Clock {
            device: device.to_string(),
            last: Mutex::new((0, 0)),
        }
    }

    /// The device its stamps end with.
    pub fn device(&self) -> &str {
        &self.device
    }

    /// A stamp later than any this clock made or saw.
    pub fn tick(&self) -> String {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let now = wall_ms();
        *last = if now > last.0 {
            (now, 0)
        } else {
            (last.0, last.1 + 1)
        };
        format!("{:013}-{:04x}-{}", last.0, last.1, self.device)
    }

    /// Moves the clock past `stamp`, seen in another device's data.
    pub fn observe(&self, stamp: &str) {
        if let Some(seen) = parse(stamp) {
            let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
            if seen > *last {
                *last = seen;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_increase_and_follow_what_was_seen() {
        let c = Clock::new("a1");
        let (s1, s2) = (c.tick(), c.tick());
        assert!(s2 > s1, "{s1} {s2}");
        assert!(s1.ends_with("-a1"));
        // Another device whose clock runs a minute ahead.
        let ahead = format!("{:013}-0000-b2", wall_ms() + 60_000);
        c.observe(&ahead);
        let s3 = c.tick();
        assert!(s3 > ahead, "{s3} must beat {ahead}");
    }
}
