//! A scroll bar over a scrolling element (GPUI draws none): a thin thumb at
//! its right edge while what it holds is longer than it is. The thumb drags;
//! a press beside it pages there. Put [`overlay`] in a `relative` parent,
//! beside the element that tracks the same [`ScrollHandle`].

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    Bounds, Context, DispatchPhase, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollHandle, Styled, WeakEntity, canvas, fill, point, px, rgba,
    size,
};

use crate::colors::colors;

/// The thumb, at rest and under the pointer or held. A thin bar stays out of
/// the way of what it is beside; a wider one is what a hand can take hold of,
/// and the moment the pointer is over it is the moment that matters. The
/// terminal's own bar (it scrolls by rows, not by a handle) takes the same.
pub(crate) const THIN: f32 = 6.;
pub(crate) const WIDE: f32 = 8.;
/// The strip that takes the press, always the width of the wider thumb and
/// more: it must not move or change size as the thumb does, or the pointer
/// would fall out of what it is aiming at.
const STRIP_W: f32 = WIDE + 8.;
const THUMB_MIN: f32 = 24.;

/// A drag of the thumb: where it started, and how far it was scrolled then.
pub type Drag = Rc<Cell<Option<(Pixels, Pixels)>>>;

/// The thumb's top and height over a view `view_h` high, scrolled `y` of
/// `most`; None when nothing scrolls.
fn thumb(view_h: Pixels, y: Pixels, most: Pixels) -> Option<(Pixels, Pixels)> {
    if most <= px(0.) || view_h <= px(0.) {
        return None;
    }
    let content = view_h + most;
    let h = (view_h * (view_h / content)).max(px(THUMB_MIN)).min(view_h);
    let top = (view_h - h) * (y / most).clamp(0., 1.);
    Some((top, h))
}

/// The bar over the element scrolled by `handle`, filling its parent.
pub fn overlay(handle: ScrollHandle, drag: Drag) -> impl gpui::IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let most = handle.max_offset().y;
            let y = -handle.offset().y;
            let view_h = bounds.size.height;
            let Some((top, h)) = thumb(view_h, y, most) else {
                return;
            };
            // A strip wider than the thumb, easy to hit. Worked out before the
            // thumb is drawn: whether the pointer is in it decides how wide
            // the thumb is.
            let strip = Bounds::new(
                point(bounds.right() - px(STRIP_W), bounds.top()),
                size(px(STRIP_W), view_h),
            );
            let hovered = strip.contains(&window.mouse_position());
            let wide = drag.get().is_some() || hovered;
            let w = px(if wide { WIDE } else { THIN });
            let bar = Bounds::new(
                point(bounds.right() - w - px(2.), bounds.top() + top),
                size(w, h),
            );
            let color = if wide { colors().soft } else { colors().muted };
            window.paint_quad(
                fill(bar, rgba(crate::colors::with_alpha(color, 0x99))).corner_radii(w / 2.),
            );
            let (down, d) = (handle.clone(), drag.clone());
            window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture
                    || e.button != MouseButton::Left
                    || !strip.contains(&e.position)
                {
                    return;
                }
                cx.stop_propagation();
                let (most, y) = (down.max_offset().y, -down.offset().y);
                let Some((top, h)) = thumb(view_h, y, most) else {
                    return;
                };
                // Off the thumb: there first.
                let at = e.position.y - strip.top();
                let y = if at < top || at > top + h {
                    let to = (at - h / 2.).max(px(0.)) / (view_h - h).max(px(1.));
                    let y = (most * to).min(most);
                    down.set_offset(point(down.offset().x, -y));
                    y
                } else {
                    y
                };
                d.set(Some((e.position.y, y)));
                window.refresh();
            });
            let (moved, d) = (handle.clone(), drag.clone());
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, _| {
                let Some((y0, s0)) = d.get().filter(|_| phase == DispatchPhase::Capture) else {
                    return;
                };
                let most = moved.max_offset().y;
                let Some((_, h)) = thumb(view_h, s0, most) else {
                    return;
                };
                let per = most / (view_h - h).max(px(1.));
                let y = (s0 + (e.position.y - y0) * per).clamp(px(0.), most);
                moved.set_offset(point(moved.offset().x, -y));
                window.refresh();
            });
            let d = drag.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, phase, window, _| {
                if phase == DispatchPhase::Capture && d.take().is_some() {
                    window.refresh();
                }
            });
            // Into the strip and out of it again: drawn afresh, so the width
            // follows the pointer. Each frame remembers what it drew, and asks
            // for another only when that stops being true.
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, _| {
                if phase == DispatchPhase::Capture && strip.contains(&e.position) != hovered {
                    window.refresh();
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Keeps hold of a thumb taken in a bar drawn by a view of its own (the
/// terminal's), for as long as the button stays down: every move of the
/// pointer, wherever it is in the window, goes to `on_drag`. An element hears
/// the pointer only while it is over it, and a quick hand is out of it in one
/// move. A move with the button up goes to `on_release`: the release itself
/// happened where nothing heard it. Paint it while the thumb is held.
pub(crate) fn hold<T: 'static>(
    view: WeakEntity<T>,
    on_drag: impl Fn(&mut T, Point<Pixels>, &mut Context<T>) + 'static,
    on_release: impl Fn(&mut T, &mut Context<T>) + 'static,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let _ = view.update(cx, |v, cx| {
                    if e.pressed_button == Some(MouseButton::Left) {
                        on_drag(v, e.position, cx);
                    } else {
                        on_release(v, cx);
                    }
                });
            });
        },
    )
    .absolute()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumb_shows_where_the_view_is() {
        assert_eq!(thumb(px(100.), px(0.), px(0.)), None, "nothing scrolls");
        // Twice as long as the view: half as high, at the top, then the bottom.
        assert_eq!(thumb(px(100.), px(0.), px(100.)), Some((px(0.), px(50.))));
        assert_eq!(
            thumb(px(100.), px(100.), px(100.)),
            Some((px(50.), px(50.)))
        );
        // Very long: never thinner than the least.
        assert_eq!(
            thumb(px(100.), px(0.), px(100_000.)).map(|t| t.1),
            Some(px(24.))
        );
    }
}
