//! A tab's panes: a binary tree of splits whose leaves are terminals.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{Axis, Bounds, Pixels};

/// One split: which way, and where the divider sits. Shared with the divider's
/// drag handler, which moves it.
pub struct SplitState {
    /// Horizontal puts the two side by side, Vertical stacks them.
    pub axis: Axis,
    /// The first part's share of the space, 0.1..=0.9.
    pub ratio: Cell<f32>,
    /// Where the split was painted, to turn a drag position into a ratio.
    pub bounds: Cell<Bounds<Pixels>>,
}

impl SplitState {
    pub fn new(axis: Axis) -> Rc<Self> {
        Rc::new(SplitState {
            axis,
            ratio: Cell::new(0.5),
            bounds: Cell::new(Bounds::default()),
        })
    }

    /// Moves the divider to `at`, a window position.
    pub fn drag_to(&self, at: gpui::Point<Pixels>) {
        let b = self.bounds.get();
        let (pos, start, len) = match self.axis {
            Axis::Horizontal => (at.x, b.origin.x, b.size.width),
            Axis::Vertical => (at.y, b.origin.y, b.size.height),
        };
        if len > Pixels::ZERO {
            self.ratio
                .set(((pos - start) / len).clamp(MIN_RATIO, 1. - MIN_RATIO));
        }
    }
}

const MIN_RATIO: f32 = 0.1;

pub enum Pane<T> {
    Leaf(T),
    Split(Rc<SplitState>, Box<Pane<T>>, Box<Pane<T>>),
}

impl<T: Clone + PartialEq> Pane<T> {
    /// The terminals, left to right and top to bottom.
    pub fn leaves(&self) -> Vec<T> {
        match self {
            Pane::Leaf(t) => vec![t.clone()],
            Pane::Split(_, a, b) => {
                let mut v = a.leaves();
                v.extend(b.leaves());
                v
            }
        }
    }

    pub fn first(&self) -> T {
        match self {
            Pane::Leaf(t) => t.clone(),
            Pane::Split(_, a, _) => a.first(),
        }
    }

    /// Replaces the leaf `target` with a split of it and `new`, `new` second.
    pub fn with_split(self, target: &T, new: T, axis: Axis) -> Pane<T> {
        match self {
            Pane::Leaf(t) if &t == target => Pane::Split(
                SplitState::new(axis),
                Box::new(Pane::Leaf(t)),
                Box::new(Pane::Leaf(new)),
            ),
            Pane::Leaf(t) => Pane::Leaf(t),
            Pane::Split(s, a, b) => {
                if a.leaves().contains(target) {
                    Pane::Split(s, Box::new(a.with_split(target, new, axis)), b)
                } else {
                    Pane::Split(s, a, Box::new(b.with_split(target, new, axis)))
                }
            }
        }
    }

    /// The tree without the leaf `target`, its sibling taking the split's place;
    /// None when `target` was the only leaf.
    pub fn without(self, target: &T) -> Option<Pane<T>> {
        match self {
            Pane::Leaf(t) if &t == target => None,
            Pane::Leaf(t) => Some(Pane::Leaf(t)),
            Pane::Split(s, a, b) => match (a.without(target), b.without(target)) {
                (Some(a), Some(b)) => Some(Pane::Split(s, Box::new(a), Box::new(b))),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// Of `others` (index, bounds), the one beside `from` in `dir`: its center must
/// lie that way, and the nearest such center wins, preferring those that
/// overlap `from` across the direction (so Right from a top pane picks the
/// pane beside it, not the one diagonally below).
pub fn neighbor(
    from: Bounds<Pixels>,
    others: &[(usize, Bounds<Pixels>)],
    dir: Dir,
) -> Option<usize> {
    let c = from.center();
    others
        .iter()
        .filter(|(_, b)| {
            let o = b.center();
            match dir {
                Dir::Left => o.x < c.x,
                Dir::Right => o.x > c.x,
                Dir::Up => o.y < c.y,
                Dir::Down => o.y > c.y,
            }
        })
        .min_by(|(_, a), (_, b)| {
            let key = |o: &Bounds<Pixels>| {
                let overlaps = match dir {
                    Dir::Left | Dir::Right => o.top() < from.bottom() && o.bottom() > from.top(),
                    Dir::Up | Dir::Down => o.left() < from.right() && o.right() > from.left(),
                };
                let (dx, dy) = (f32::from(o.center().x - c.x), f32::from(o.center().y - c.y));
                (!overlaps, dx * dx + dy * dy)
            };
            let (ka, kb) = (key(a), key(b));
            ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
        })
        .map(|(i, _)| *i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    #[test]
    fn split_and_remove_keep_the_order() {
        let p = Pane::Leaf(1).with_split(&1, 2, Axis::Horizontal);
        let p = p.with_split(&2, 3, Axis::Vertical);
        assert_eq!(p.leaves(), [1, 2, 3]);
        let p = p.without(&2).expect("two left");
        assert_eq!(p.leaves(), [1, 3]);
        let p = p.without(&1).expect("one left");
        assert!(matches!(p, Pane::Leaf(3)));
        assert!(p.without(&3).is_none());
    }

    fn b(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    #[test]
    fn neighbors_prefer_panes_beside() {
        // Left half; right half split into top and bottom.
        let left = b(0., 0., 400., 600.);
        let top = b(400., 0., 400., 300.);
        let bottom = b(400., 300., 400., 300.);
        let all = [(0, left), (1, top), (2, bottom)];
        assert_eq!(neighbor(left, &all[1..], Dir::Right), Some(1));
        assert_eq!(neighbor(bottom, &[all[0], all[1]], Dir::Up), Some(1));
        assert_eq!(neighbor(bottom, &[all[0], all[1]], Dir::Left), Some(0));
        assert_eq!(neighbor(top, &[all[0], all[2]], Dir::Right), None);
    }

    #[test]
    fn dragging_clamps_the_ratio() {
        let s = SplitState::new(Axis::Horizontal);
        s.bounds.set(b(100., 0., 800., 600.));
        s.drag_to(point(px(300.), px(10.)));
        assert_eq!(s.ratio.get(), 0.25);
        s.drag_to(point(px(0.), px(10.)));
        assert_eq!(s.ratio.get(), MIN_RATIO);
    }
}
