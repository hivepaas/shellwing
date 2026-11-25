//! Files dragged in from another app (a file manager) and dropped on an
//! element.
//!
//! GPUI's own `on_drop` takes a drop only on an element under the pointer,
//! and an element is not, to GPUI, while the last input was the keyboard: a
//! drag from another app moves no mouse in this window, so files dragged in
//! right after typing (`ls`, Enter) were dropped on nothing. This follows
//! the drag by its moves instead, which GPUI gives every element, and takes
//! the drop itself where the drag last was.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, Bounds, Context, DispatchPhase, ExternalPaths, InteractiveElement, MouseUpEvent,
    ParentElement, Pixels, Stateful, Styled, Window, canvas,
};

/// Files dragged over an element: its bounds, and the files.
type Over = Option<(Bounds<Pixels>, Vec<PathBuf>)>;

/// Where files being dragged in are, for one element: over it, or not.
#[derive(Clone, Default)]
pub struct DropIn(Rc<RefCell<Over>>);

impl DropIn {
    /// Whether files are being dragged over the element now.
    pub fn over(&self) -> bool {
        self.0.borrow().is_some()
    }

    /// A drag that left the window, or ended elsewhere, leaves nothing over
    /// it: called as the view renders.
    pub fn settle(&self, cx: &App) {
        if !cx.has_active_drag() {
            self.0.borrow_mut().take();
        }
    }
}

/// Makes `el` take files dropped on it: `dropped` gets them, on the view.
pub fn drop_in<V: 'static>(
    el: Stateful<gpui::Div>,
    state: &DropIn,
    cx: &mut Context<V>,
    dropped: fn(&mut V, Vec<PathBuf>, &mut Window, &mut Context<V>),
) -> Stateful<gpui::Div> {
    let moved = state.clone();
    let view = cx.entity().downgrade();
    let up = state.clone();
    el.on_drag_move::<ExternalPaths>(move |e, window, cx| {
        let inside = e.bounds.contains(&e.event.position);
        let was = moved.over();
        *moved.0.borrow_mut() = inside.then(|| (e.bounds, e.drag(cx).paths().to_vec()));
        if was != inside {
            window.refresh();
        }
    })
    .child(
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                if !up.over() {
                    return;
                }
                let up = up.clone();
                let view = view.clone();
                window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    let Some((bounds, paths)) = up.0.borrow_mut().take() else {
                        return;
                    };
                    if !bounds.contains(&e.position) {
                        return;
                    }
                    cx.stop_active_drag(window);
                    cx.stop_propagation();
                    let _ = view.update(cx, |v, cx| dropped(v, paths, window, cx));
                });
            },
        )
        .absolute()
        .size_0(),
    )
}
