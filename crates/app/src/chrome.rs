//! Client-side window decorations.
//!
//! GNOME's Wayland compositor draws no frame for clients, so the window provides
//! its own title bar (drag to move, double-click to maximise, right-click for the
//! window menu, buttons), a thin border and a shadow.
//!
//! Resizing works as in GTK apps: an invisible margin (the client inset) runs
//! around the visible window, and pressing there resizes. Near a corner a longer
//! stretch of each edge resizes diagonally, so corners are easy to grab. Tiled
//! or maximised sides have neither margin nor border.

use gpui::{
    App, BoxShadow, CursorStyle, Decorations, Div, HitboxBehavior, Hsla, MouseButton, Pixels,
    Point, ResizeEdge, Size, Tiling, Window, canvas, div, hsla, point, prelude::*, px, rgb, svg,
    transparent_black,
};

/// The header bar: as tall as GNOME's own where the app draws it (Linux),
/// a row for the tabs under the native title bar elsewhere.
pub const TITLEBAR_H: f32 = if cfg!(any(target_os = "linux", target_os = "freebsd")) {
    44.0
} else {
    34.0
};
/// The invisible margin outside the visible window that resizes it; the shadow
/// is drawn into it.
const INSET: f32 = 12.0;
const BORDER: f32 = 1.0;
/// Along each edge, this far from the window's corner resizes diagonally.
const CORNER: f32 = 28.0;

fn tiling(window: &Window) -> Tiling {
    match window.window_decorations() {
        Decorations::Client { tiling } => tiling,
        Decorations::Server => Tiling::tiled(),
    }
}

/// The window frame around `content`, with `tabs` in the title bar.
pub fn window_chrome(tabs: Div, content: Div, bg: Hsla, window: &mut Window) -> Div {
    let t = tiling(window);
    window.set_client_inset(px(INSET));
    // Round buttons with symbolic icons, as GNOME draws them.
    let button = |id: &'static str, icon: &'static str, f: fn(&mut Window, &mut App)| {
        div()
            .id(id)
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(rgb(0x1a2333))
            .hover(|s| s.bg(rgb(0x2a3548)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| f(window, cx))
            .child(svg().path(icon).size(px(14.)).text_color(rgb(0xd5dbe5)))
    };
    let titlebar = div()
        .h(px(TITLEBAR_H))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .bg(rgb(0x0f1522))
        .border_b_1()
        .border_color(rgb(0x1e2736))
        .text_color(rgb(0xa1a1aa))
        .text_sm()
        .on_mouse_down(MouseButton::Left, |e, window, _| {
            if e.click_count == 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        })
        .on_mouse_down(MouseButton::Right, |e, window, _| {
            window.show_window_menu(e.position)
        })
        .child(div().flex_1().min_w_0().h_full().child(tabs))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .h_full()
                .pl(px(6.))
                .pr(px(10.))
                .child(button("min", "icons/minimize.svg", |w, _| {
                    w.minimize_window()
                }))
                .child(button("max", "icons/maximize.svg", |w, _| w.zoom_window()))
                .child(button(
                    "close",
                    "icons/close.svg",
                    crate::workspace::close_window,
                )),
        );
    let (inset, border) = (px(INSET), px(BORDER));
    let floating = !(t.top && t.bottom && t.left && t.right);
    div()
        .size_full()
        .bg(transparent_black())
        .when(!t.top, |d| d.pt(inset))
        .when(!t.bottom, |d| d.pb(inset))
        .when(!t.left, |d| d.pl(inset))
        .when(!t.right, |d| d.pr(inset))
        .child(
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                move |bounds, hitbox, window, _| {
                    if let Some(e) = resize_edge(window.mouse_position(), bounds.size, t) {
                        use ResizeEdge::*;
                        let style = match e {
                            Top | Bottom => CursorStyle::ResizeUpDown,
                            Left | Right => CursorStyle::ResizeLeftRight,
                            TopLeft | BottomRight => CursorStyle::ResizeUpLeftDownRight,
                            TopRight | BottomLeft => CursorStyle::ResizeUpRightDownLeft,
                        };
                        window.set_cursor_style(style, &hitbox);
                    }
                },
            )
            .size_full()
            .absolute()
            .top_0()
            .left_0(),
        )
        .on_mouse_move(|_, window, _| window.refresh())
        .on_mouse_down(MouseButton::Left, move |e, window, _| {
            if let Some(edge) = resize_edge(e.position, window.viewport_size(), t) {
                window.start_window_resize(edge);
            }
        })
        .child(
            div()
                .size_full()
                .flex()
                .flex_col()
                .bg(bg)
                .border_color(rgb(0x3f3f46))
                .when(!t.top, |d| d.border_t(border))
                .when(!t.bottom, |d| d.border_b(border))
                .when(!t.left, |d| d.border_l(border))
                .when(!t.right, |d| d.border_r(border))
                .when(floating, |d| {
                    d.shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., 0.45),
                        offset: point(px(0.), px(2.)),
                        blur_radius: px(INSET - 2.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                })
                .child(titlebar)
                .child(content),
        )
}

/// Which way a press at `pos` resizes a window of `size`, if at all: anywhere in
/// the margin and border of an untiled side, diagonally near a corner.
fn resize_edge(pos: Point<Pixels>, size: Size<Pixels>, t: Tiling) -> Option<ResizeEdge> {
    use ResizeEdge::*;
    let grab = px(INSET + BORDER);
    let corner = px(INSET + CORNER);
    let (l, r) = (
        !t.left && pos.x < grab,
        !t.right && pos.x > size.width - grab,
    );
    let (top, b) = (
        !t.top && pos.y < grab,
        !t.bottom && pos.y > size.height - grab,
    );
    let (near_l, near_r) = (
        !t.left && pos.x < corner,
        !t.right && pos.x > size.width - corner,
    );
    let (near_t, near_b) = (
        !t.top && pos.y < corner,
        !t.bottom && pos.y > size.height - corner,
    );
    Some(if (top && near_l) || (l && near_t) {
        TopLeft
    } else if (top && near_r) || (r && near_t) {
        TopRight
    } else if (b && near_l) || (l && near_b) {
        BottomLeft
    } else if (b && near_r) || (r && near_b) {
        BottomRight
    } else if top {
        Top
    } else if b {
        Bottom
    } else if l {
        Left
    } else if r {
        Right
    } else {
        return None;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    const W: f32 = 800.;
    const H: f32 = 600.;

    fn edge(x: f32, y: f32, t: Tiling) -> Option<ResizeEdge> {
        resize_edge(point(px(x), px(y)), size(px(W), px(H)), t)
    }

    fn floating() -> Tiling {
        Tiling::default()
    }

    #[test]
    fn corners_take_a_long_stretch_of_each_edge() {
        let t = floating();
        // In the margin below the bottom edge, 30 px left of the corner.
        assert_eq!(edge(W - 30., H - 4., t), Some(ResizeEdge::BottomRight));
        // In the margin right of the right edge, 30 px above the corner.
        assert_eq!(edge(W - 4., H - 30., t), Some(ResizeEdge::BottomRight));
        assert_eq!(edge(5., 5., t), Some(ResizeEdge::TopLeft));
        assert_eq!(edge(W - 30., 3., t), Some(ResizeEdge::TopRight));
        // Further along, a plain edge.
        assert_eq!(edge(W / 2., H - 4., t), Some(ResizeEdge::Bottom));
        assert_eq!(edge(4., H / 2., t), Some(ResizeEdge::Left));
    }

    #[test]
    fn content_never_resizes() {
        let t = floating();
        assert_eq!(
            edge(W - INSET - BORDER - 1., H - INSET - BORDER - 1., t),
            None
        );
        assert_eq!(edge(W / 2., H / 2., t), None);
    }

    #[test]
    fn tiled_sides_do_not_resize() {
        let t = Tiling {
            right: true,
            bottom: true,
            ..Tiling::default()
        };
        assert_eq!(edge(W - 2., H - 2., t), None);
        assert_eq!(edge(2., H - 2., t), Some(ResizeEdge::Left));
        assert_eq!(edge(W - 2., H / 2., Tiling::tiled()), None);
    }
}
