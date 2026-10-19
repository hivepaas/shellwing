//! Questions that need an answer before going on (quitting, closing a pane
//! that is connected), drawn in the app's look. Linux has no dialog of its
//! own for GPUI to use, and GPUI's stand-in is a white box that fits nothing
//! here; macOS and Windows keep their own.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton,
    PromptButton, PromptHandle, PromptLevel, PromptResponse, Render, RenderablePromptHandle,
    SharedString, Window, div, prelude::*, px, rgb, rgba,
};

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
    focus: FocusHandle,
}

impl EventEmitter<PromptResponse> for AppPrompt {}

impl Focusable for AppPrompt {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl AppPrompt {
    /// Enter's answer: the first that is not Cancel.
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

    fn on_key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let answer = match ev.keystroke.key.as_str() {
            "enter" => self.main(),
            "escape" => self.cancel(),
            _ => return,
        };
        if let Some(ix) = answer {
            cx.emit(PromptResponse(ix));
        }
        cx.stop_propagation();
    }
}

impl Render for AppPrompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let font = cx.try_global::<PromptFont>().and_then(|f| f.0.clone());
        let mark = match self.level {
            PromptLevel::Info => None,
            PromptLevel::Warning => Some((0xf59e0b, 0x2a1d07)),
            PromptLevel::Critical => Some((0xf87171, 0x2a1215)),
        };
        // Cancel first, at the left of the row; the main answer last.
        let main = self.main();
        let mut order: Vec<usize> = (0..self.actions.len()).collect();
        order.sort_by_key(|&i| (Some(i) == main, !self.actions[i].is_cancel()));
        let mut buttons = div().flex().justify_end().gap(px(8.)).pt(px(4.));
        for ix in order {
            let label = self.actions[ix].label().to_string();
            let id = format!("prompt-{ix}");
            let button = if Some(ix) == main {
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
            .w(px(440.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(24.))
            .py(px(22.))
            .rounded(px(12.))
            .bg(rgb(0x0f1522))
            .border_1()
            .border_color(rgb(0x2b3648))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(title)
            .when_some(self.detail.clone(), |d, detail| {
                d.child(
                    div()
                        .text_size(ui(13.))
                        .text_color(rgb(0xa3aec0))
                        .child(detail),
                )
            })
            .child(buttons);
        div()
            .id("prompt")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x03060ca8))
            .text_size(ui(13.))
            .text_color(rgb(0xe2e8f0))
            .when_some(font, |d, f| d.font_family(f))
            .cursor_default()
            .child(card)
    }
}

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
    let view = cx.new(|cx| AppPrompt {
        level,
        message: message.to_string(),
        detail: detail.map(str::to_string),
        actions: actions.to_vec(),
        focus: cx.focus_handle(),
    });
    handle.with_view(view, window, cx)
}
