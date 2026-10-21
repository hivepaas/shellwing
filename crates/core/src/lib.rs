//! What a shellwing window shows, without the window: sessions and their
//! terminals, the prompts they ask through, saved secrets and host targets.
//! The desktop app draws it with GPUI; other front ends can use it as is.

pub mod bench;
pub mod command;
pub mod images;
pub mod local;
pub mod logs;
pub mod secrets;
pub mod session;
pub mod target;
pub mod writer;

pub use writer::Writer;
