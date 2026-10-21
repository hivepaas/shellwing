//! Notices: what the app says for a while at the edge of a window, in the
//! colors of what it is (information, good news, a warning, a failure), so it
//! stands out from the page under it.

use gpui::{Div, SharedString, Styled, rgb};

use crate::colors::colors;

/// What kind of thing a notice says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Info,
    /// Done as asked.
    Good,
    /// Not done, or worth a look, without anything failing.
    Warn,
    /// Something failed.
    Bad,
}

/// A notice's text and tone. A bare string is information.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub text: SharedString,
    pub tone: Tone,
}

impl Notice {
    pub fn good(text: impl Into<SharedString>) -> Notice {
        Notice {
            text: text.into(),
            tone: Tone::Good,
        }
    }

    pub fn warn(text: impl Into<SharedString>) -> Notice {
        Notice {
            text: text.into(),
            tone: Tone::Warn,
        }
    }

    pub fn bad(text: impl Into<SharedString>) -> Notice {
        Notice {
            text: text.into(),
            tone: Tone::Bad,
        }
    }
}

impl From<String> for Notice {
    fn from(text: String) -> Notice {
        Notice {
            text: text.into(),
            tone: Tone::Info,
        }
    }
}

impl From<SharedString> for Notice {
    fn from(text: SharedString) -> Notice {
        Notice {
            text,
            tone: Tone::Info,
        }
    }
}

/// `d` in the colors of `tone`: its ground, edge and text.
pub fn colored(d: Div, tone: Tone) -> Div {
    let c = colors();
    let (ground, edge, text) = match tone {
        Tone::Info => (c.info_bg, c.info_edge, c.info_text),
        Tone::Good => (c.good_bg, c.good_edge, c.good_text),
        Tone::Warn => (c.warn_bg, c.warn_edge, c.warn_text),
        Tone::Bad => (c.bad_bg, c.bad_edge, c.bad_text),
    };
    d.bg(rgb(ground))
        .border_1()
        .border_color(rgb(edge))
        .text_color(rgb(text))
}
