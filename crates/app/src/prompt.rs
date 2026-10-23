//! Questions that need an answer before going on (quitting, closing a pane
//! that is connected), drawn in the app's look on every system, so they read
//! the same on macOS, Windows and Linux (which has no dialog of its own for
//! GPUI to use).

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton,
    PromptButton, PromptHandle, PromptLevel, PromptResponse, Render, RenderablePromptHandle,
    SharedString, WeakEntity, Window, div, prelude::*, px, rgb, rgba,
};

use crate::colors::colors;
use crate::host_editor::{outline_button, primary_button};
use crate::look::ui;

/// The UI font, set by the workspace: a prompt is drawn outside its tree.
#[derive(Default)]
pub struct PromptFont(pub Option<SharedString>);

impl gpui::Global for PromptFont {}

struct AppPrompt {
    level: PromptLevel,
    message: String,
    detail: Option<String>,
    actions: Vec<PromptButton>,
    /// The button Enter presses, drawn as the main one: the main answer at
    /// first; Tab moves it along the row.
    chosen: usize,
    focus: FocusHandle,
}

impl EventEmitter<PromptResponse> for AppPrompt {}

impl Focusable for AppPrompt {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl AppPrompt {
    /// The main answer, chosen at first: the first that is not Cancel.
    fn main(&self) -> Option<usize> {
        self.actions.iter().position(|a| !a.is_cancel())
    }

    /// Escape's answer: Cancel, else the last.
    fn cancel(&self) -> Option<usize> {
        self.actions
            .iter()
            .position(PromptButton::is_cancel)
            .or(self.actions.len().checked_sub(1))
    }

    /// The buttons as drawn, left to right: Cancel first, the main answer
    /// last.
    fn order(&self) -> Vec<usize> {
        let main = self.main();
        let mut order: Vec<usize> = (0..self.actions.len()).collect();
        order.sort_by_key(|&i| (Some(i) == main, !self.actions[i].is_cancel()));
        order
    }

    /// The next button along the row (`back`: the one before), round.
    fn step(&mut self, back: bool) {
        let order = self.order();
        let n = order.len();
        if n == 0 {
            return;
        }
        let at = order.iter().position(|&i| i == self.chosen).unwrap_or(0);
        self.chosen = order[if back { (at + n - 1) % n } else { (at + 1) % n }];
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let answer = match k.key.as_str() {
            "enter" => (self.chosen < self.actions.len()).then_some(self.chosen),
            "escape" => self.cancel(),
            "tab" | "left" | "right" => {
                self.step(k.key == "left" || (k.key == "tab" && k.modifiers.shift));
                cx.notify();
                None
            }
            _ => return,
        };
        if let Some(ix) = answer {
            cx.emit(PromptResponse(ix));
        }
        cx.stop_propagation();
    }
}

impl Render for AppPrompt {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // It has the keys while it is up: what took them meanwhile (a pane
        // focused as another closed) gives them back.
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        let font = cx.try_global::<PromptFont>().and_then(|f| f.0.clone());
        let mark = match self.level {
            PromptLevel::Info => None,
            PromptLevel::Warning => Some((colors().amber, colors().warn_bg)),
            PromptLevel::Critical => Some((colors().bad, colors().bad_bg)),
        };
        // Cancel first, at the left of the row; the main answer last. The
        // one Enter presses is drawn as the main one.
        let mut buttons = div().flex().justify_end().gap(px(8.)).pt(px(4.));
        for ix in self.order() {
            let label = self.actions[ix].label().to_string();
            let id = format!("prompt-{ix}");
            let button = if ix == self.chosen {
                primary_button(&id, &label)
            } else {
                outline_button(&id, &label)
            };
            buttons = buttons.child(button.on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(PromptResponse(ix));
                cx.stop_propagation();
            })));
        }
        let title = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .when_some(mark, |d, (fg, bg)| {
                d.child(
                    div()
                        .size(px(24.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(rgb(bg))
                        .text_color(rgb(fg))
                        .font_weight(FontWeight::BOLD)
                        .child("!"),
                )
            })
            .child(
                div()
                    .text_size(ui(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.message.clone()),
            );
        let card = div()
            .id("prompt-card")
            .debug_selector(|| "prompt-card".into())
            .w(px(440.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(24.))
            .py(px(22.))
            .rounded(px(12.))
            .bg(rgb(colors().surface))
            .border_1()
            .border_color(rgb(colors().edge))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(title)
            .when_some(self.detail.clone(), |d, detail| {
                d.child(
                    div()
                        .text_size(ui(13.))
                        .text_color(rgb(colors().label))
                        .child(detail),
                )
            })
            .child(buttons);
        // It covers the whole surface, the margin around the window too: the
        // shade stops at the window's edge and follows its corners.
        let (margin, round) = (
            crate::chrome::margins(window),
            crate::chrome::corners(window),
        );
        div()
            .id("prompt")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            // Nothing under it takes a click, nor so the keys after it.
            .occlude()
            .size_full()
            .pt(margin.top)
            .pr(margin.right)
            .pb(margin.bottom)
            .pl(margin.left)
            .text_size(ui(13.))
            .text_color(rgb(colors().text))
            .when_some(font, |d, f| d.font_family(f))
            .cursor_default()
            .child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_tl(round.top_left)
                    .rounded_tr(round.top_right)
                    .rounded_bl(round.bottom_left)
                    .rounded_br(round.bottom_right)
                    .bg(rgba(colors().scrim))
                    .child(card),
            )
    }
}

/// The questions open, in every window: see [`dismiss_all`].
#[derive(Default)]
struct Open(Vec<WeakEntity<AppPrompt>>);

impl gpui::Global for Open {}

/// For [`App::set_prompt_builder`].
pub fn render(
    level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    actions: &[PromptButton],
    handle: PromptHandle,
    window: &mut Window,
    cx: &mut App,
) -> RenderablePromptHandle {
    let view = cx.new(|cx| {
        let mut p = AppPrompt {
            level,
            message: message.to_string(),
            detail: detail.map(str::to_string),
            actions: actions.to_vec(),
            chosen: 0,
            focus: cx.focus_handle(),
        };
        p.chosen = p.main().unwrap_or(0);
        p
    });
    let open = &mut cx.default_global::<Open>().0;
    open.retain(|p| p.upgrade().is_some());
    open.push(view.downgrade());
    handle.with_view(view, window, cx)
}

/// Answers every question open as Escape does: shellwing locking, nobody
/// may answer one drawn over the lock (letting a device into the vault,
/// deleting a note).
pub fn dismiss_all(cx: &mut App) {
    let open = std::mem::take(&mut cx.default_global::<Open>().0);
    for p in open.into_iter().filter_map(|p| p.upgrade()) {
        p.update(cx, |p, cx| {
            if let Some(ix) = p.cancel() {
                cx.emit(PromptResponse(ix));
            }
        });
    }
}
