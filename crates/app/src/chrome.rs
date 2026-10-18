//! Client-side window decorations.
//!
//! GNOME's Wayland compositor draws no frame for clients, so the window provides
//! its own title bar (drag to move, double-click to maximise, right-click for the
//! window menu, buttons) and a thin border that resizes the window.

use gpui::{
    App, Bounds, CursorStyle, Div, HitboxBehavior, Hsla, MouseButton, Pixels, Point, ResizeEdge,
    Size, Window, canvas, div, point, prelude::*, px, rgb,
};

pub const RESIZE_EDGE: f32 = 6.0;
pub const TITLEBAR_H: f32 = 32.0;

pub fn window_chrome(title: String, content: Div, bg: Hsla) -> Div {
    let edge = px(RESIZE_EDGE);
    let button = |id: &'static str, label: &'static str, f: fn(&mut Window, &mut App)| {
        div()
            .id(id)
            .w(px(36.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(0xd4d4d8))
            .hover(|s| s.bg(rgb(0x3f3f46)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| f(window, cx))
            .child(label)
    };
    let titlebar = div()
        .h(px(TITLEBAR_H))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .pl_3()
        .bg(rgb(0x27272a))
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
        .child(title)
        .child(
            div()
                .flex()
                .h_full()
                .child(button("min", "—", |w, _| w.minimize_window()))
                .child(button("max", "□", |w, _| w.zoom_window()))
                .child(button("close", "✕", |w, _| w.remove_window())),
        );
    div()
        .size_full()
        .p(edge)
        .bg(rgb(0x3f3f46))
        .child(
            canvas(
                |_bounds, window, _| {
                    window.insert_hitbox(
                        Bounds::new(
                            point(px(0.), px(0.)),
                            window.window_bounds().get_bounds().size,
                        ),
                        HitboxBehavior::Normal,
                    )
                },
                move |_bounds, hitbox, window, _| {
                    let size = window.window_bounds().get_bounds().size;
                    if let Some(e) = resize_edge(window.mouse_position(), edge, size) {
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
            .absolute(),
        )
        .on_mouse_move(|_, window, _| window.refresh())
        .on_mouse_down(MouseButton::Left, move |e, window, _| {
            let size = window.window_bounds().get_bounds().size;
            if let Some(edge) = resize_edge(e.position, edge, size) {
                window.start_window_resize(edge);
            }
        })
        .child(
            div()
                .size_full()
                .flex()
                .flex_col()
                .bg(bg)
                .child(titlebar)
                .child(content),
        )
}

fn resize_edge(pos: Point<Pixels>, e: Pixels, size: Size<Pixels>) -> Option<ResizeEdge> {
    use ResizeEdge::*;
    let (l, r, t, b) = (
        pos.x < e,
        pos.x > size.width - e,
        pos.y < e,
        pos.y > size.height - e,
    );
    Some(match (t, b, l, r) {
        (true, _, true, _) => TopLeft,
        (true, _, _, true) => TopRight,
        (_, true, true, _) => BottomLeft,
        (_, true, _, true) => BottomRight,
        (true, ..) => Top,
        (_, true, ..) => Bottom,
        (_, _, true, _) => Left,
        (_, _, _, true) => Right,
        _ => return None,
    })
}
