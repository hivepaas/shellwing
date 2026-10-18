//! Where the window opens: where it was last left, if that is still on a screen.
//!
//! Wayland does not let a client know or choose its position, so there only the
//! size and the maximised or fullscreen state come back; the compositor places
//! the window.

use gpui::{App, Bounds, Pixels, WindowBounds, point, px, size};
use shellwing_config::{WindowMode, WindowState};

const DEFAULT_SIZE: (f32, f32) = (1100., 720.);
const MIN_SIZE: (f32, f32) = (400., 240.);
/// How much of the window must be on a display to keep its old position: enough
/// to grab the title bar.
const MIN_VISIBLE: (f32, f32) = (120., 40.);

pub fn initial(saved: Option<WindowState>, cx: &App) -> WindowBounds {
    let Some(s) = saved else {
        return WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(DEFAULT_SIZE.0), px(DEFAULT_SIZE.1)),
            cx,
        ));
    };
    let wanted = Bounds::new(
        point(px(s.x), px(s.y)),
        size(px(s.width.max(MIN_SIZE.0)), px(s.height.max(MIN_SIZE.1))),
    );
    let displays: Vec<Bounds<Pixels>> = cx.displays().iter().map(|d| d.bounds()).collect();
    let bounds = if on_a_display(wanted, &displays) {
        wanted
    } else {
        // The display it was on is gone: keep the size, center it.
        Bounds::centered(None, wanted.size, cx)
    };
    match s.mode {
        WindowMode::Windowed => WindowBounds::Windowed(bounds),
        WindowMode::Maximized => WindowBounds::Maximized(bounds),
        WindowMode::Fullscreen => WindowBounds::Fullscreen(bounds),
    }
}

fn on_a_display(window: Bounds<Pixels>, displays: &[Bounds<Pixels>]) -> bool {
    displays.iter().any(|d| {
        let v = d.intersect(&window).size;
        v.width >= px(MIN_VISIBLE.0) && v.height >= px(MIN_VISIBLE.1)
    })
}

pub fn state(bounds: WindowBounds) -> WindowState {
    let (b, mode) = match bounds {
        WindowBounds::Windowed(b) => (b, WindowMode::Windowed),
        WindowBounds::Maximized(b) => (b, WindowMode::Maximized),
        WindowBounds::Fullscreen(b) => (b, WindowMode::Fullscreen),
    };
    WindowState {
        x: f32::from(b.origin.x),
        y: f32::from(b.origin.y),
        width: f32::from(b.size.width),
        height: f32::from(b.size.height),
        mode,
        sidebar_width: None,
        sidebar_collapsed: false,
        files_width: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    #[test]
    fn keeps_a_position_only_on_a_display() {
        let screens = [b(0., 0., 1920., 1080.), b(1920., 0., 2560., 1440.)];
        assert!(
            on_a_display(b(2000., 100., 800., 600.), &screens),
            "second screen"
        );
        assert!(
            on_a_display(b(1800., 100., 800., 600.), &screens),
            "across both"
        );
        // The second screen was unplugged.
        assert!(!on_a_display(b(2000., 100., 800., 600.), &screens[..1]));
        // Only a sliver left on screen.
        assert!(!on_a_display(b(1900., 100., 800., 600.), &screens[..1]));
    }

    #[test]
    fn state_keeps_the_restore_bounds_and_mode() {
        let s = state(WindowBounds::Maximized(b(10., 20., 1200., 800.)));
        assert_eq!((s.x, s.y, s.width, s.height), (10., 20., 1200., 800.));
        assert_eq!(s.mode, WindowMode::Maximized);
    }
}
