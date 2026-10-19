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
//!
//! The window's corners are round where both their sides are free. GPUI clips
//! children to rectangles only, so what reaches a corner rounds its own: the
//! title bar the top ones, and at the bottom whatever the workspace puts there
//! (see [`BottomCorners`]).

use gpui::{
    App, BoxShadow, Corners, CursorStyle, Decorations, Div, Edges, HitboxBehavior, Hsla,
    MouseButton, Pixels, Point, ResizeEdge, Size, Tiling, Window, canvas, div, point, prelude::*,
    px, rgb, rgba, svg, transparent_black,
};

use crate::colors::colors;

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
/// How round the window's corners are, as GNOME rounds its own.
pub const RADIUS: f32 = 10.0;

/// The radius of each corner of the window: [`RADIUS`] where the app draws
/// the frame and both sides of the corner are free (not tiled, not
/// maximised); none where the system draws the frame, which rounds its own.
pub fn corners(window: &Window) -> Corners<Pixels> {
    let Decorations::Client { tiling: t } = window.window_decorations() else {
        return Corners::default();
    };
    let round = |a: bool, b: bool| if a || b { px(0.) } else { px(RADIUS) };
    Corners {
        top_left: round(t.top, t.left),
        top_right: round(t.top, t.right),
        bottom_left: round(t.bottom, t.left),
        bottom_right: round(t.bottom, t.right),
    }
}

/// The invisible margin outside the visible window, on each side: none on a
/// tiled side, and none where the system draws the frame.
pub fn margins(window: &Window) -> Edges<Pixels> {
    let Decorations::Client { tiling: t } = window.window_decorations() else {
        return Edges::default();
    };
    let side = |tiled: bool| if tiled { px(0.) } else { px(INSET) };
    Edges {
        top: side(t.top),
        right: side(t.right),
        bottom: side(t.bottom),
        left: side(t.left),
    }
}

/// The shade a dialog puts over the whole window, and where it goes (for
/// `anchored().position`): the surface is larger than the window, by the
/// margin the shadow is drawn in, so it stops at the window's edge and
/// follows its corners.
pub fn shade(window: &Window) -> (Point<Pixels>, Div) {
    let size = window.viewport_size();
    let (m, round) = (margins(window), corners(window));
    let shade = div()
        .w(size.width - m.left - m.right)
        .h(size.height - m.top - m.bottom)
        .rounded_tl(round.top_left)
        .rounded_tr(round.top_right)
        .rounded_bl(round.bottom_left)
        .rounded_br(round.bottom_right)
        .bg(rgba(colors().scrim));
    (point(m.left, m.top), shade)
}

/// The bottom corners of the window's body, inside its border: what fills
/// one of them rounds it by this much. The workspace sets it each frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BottomCorners {
    pub left: Pixels,
    pub right: Pixels,
}

impl gpui::Global for BottomCorners {}

impl BottomCorners {
    /// The window's, inside the 1 px border.
    pub fn of(window: &Window) -> Self {
        let c = corners(window);
        let inner = |r: Pixels| if r > px(0.) { r - px(BORDER) } else { r };
        BottomCorners {
            left: inner(c.bottom_left),
            right: inner(c.bottom_right),
        }
    }

    /// The window's, as the workspace last set them.
    pub fn current(cx: &App) -> Self {
        cx.try_global::<BottomCorners>()
            .copied()
            .unwrap_or_default()
    }

    /// Only the left one: for what fills the left of the body.
    pub fn left(self) -> Self {
        BottomCorners {
            right: px(0.),
            ..self
        }
    }

    /// Only the right one.
    pub fn right(self) -> Self {
        BottomCorners {
            left: px(0.),
            ..self
        }
    }

    pub fn none() -> Self {
        Self::default()
    }

    /// Rounds the bottom corners of `el`.
    pub fn round<E: Styled>(self, el: E) -> E {
        el.rounded_bl(self.left).rounded_br(self.right)
    }
}

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
    let round = corners(window);
    let inner = |r: Pixels| if r > px(0.) { r - px(BORDER) } else { r };
    // Round buttons with symbolic icons, as GNOME draws them.
    let button = |id: &'static str, icon: &'static str, f: fn(&mut Window, &mut App)| {
        div()
            .id(id)
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(rgb(colors().button))
            .hover(|s| s.bg(rgb(colors().button_hover)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| f(window, cx))
            .child(
                svg()
                    .path(icon)
                    .size(px(14.))
                    .text_color(rgb(colors().text)),
            )
    };
    let titlebar = div()
        .h(px(TITLEBAR_H))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .rounded_tl(inner(round.top_left))
        .rounded_tr(inner(round.top_right))
        .bg(rgb(colors().chrome))
        .border_b_1()
        .border_color(rgb(colors().line))
        .text_color(rgb(colors().soft))
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
                .rounded_tl(round.top_left)
                .rounded_tr(round.top_right)
                .rounded_bl(round.bottom_left)
                .rounded_br(round.bottom_right)
                .border_color(rgb(colors().window_edge))
                .when(!t.top, |d| d.border_t(border))
                .when(!t.bottom, |d| d.border_b(border))
                .when(!t.left, |d| d.border_l(border))
                .when(!t.right, |d| d.border_r(border))
                .when(floating, |d| {
                    d.shadow(vec![BoxShadow {
                        color: rgba(colors().shadow).into(),
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
