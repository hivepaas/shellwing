//! The About page: the version, where the code is and how to take part,
//! updates, and HivePaaS, from the same author.

use gpui::{
    ClipboardItem, Context, FontWeight, InteractiveElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::*, px, rgb, svg,
};
use shellwing_i18n::tr;

use super::{SettingValue, SettingsView, heading, label, page, small_button, toggle};
use crate::colors::colors;
use crate::look::ui;
use crate::palette::Action;
use crate::workspace::{UpdateStage, Workspace};

const SITE: &str = "https://shellwing.app";
const REPO: &str = "https://github.com/hivepaas/shellwing";
const HIVEPAAS: &str = "https://hivepaas.com";
const HIVEPAAS_REPO: &str = "https://github.com/hivepaas/hivepaas";
/// The year in the copyright line.
const YEAR: u32 = 2026;

/// What a problem report needs to say about this build.
fn version_info() -> String {
    format!(
        "shellwing {} ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// A link to a page of the shellwing repository.
fn repo(path: &str) -> String {
    format!("{REPO}/{path}")
}

impl SettingsView {
    pub(super) fn about(&self, cx: &mut Context<Self>) -> gpui::Div {
        page(&tr!("About"))
            .child(self.identity(cx))
            .child(open_source(cx))
            .child(self.updates(cx))
            .child(hivepaas(cx))
            .child(
                div()
                    .pt(px(12.))
                    .border_t_1()
                    .border_color(rgb(colors().line))
                    .child(label(&tr!(
                        "© {year} HivePaaS. The shellwing name and logo are trademarks of HivePaaS.",
                        year = YEAR.to_string()
                    ))),
            )
    }

    /// The logo, the name, what it is and which build this is.
    fn identity(&self, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(16.))
            .child(app_logo())
            .child(
                div()
                    .flex_1()
                    .min_w(px(220.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(ui(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("shellwing"),
                    )
                    .child(
                        div()
                            .text_size(ui(14.))
                            .text_color(rgb(colors().soft))
                            .child(tr!("A native SSH client for Linux, macOS and Windows.")),
                    )
                    .child(
                        div()
                            .pt(px(2.))
                            .text_size(ui(12.))
                            .text_color(rgb(colors().muted))
                            .child(tr!(
                                "Version {version} · {os} {arch}",
                                version = env!("CARGO_PKG_VERSION"),
                                os = std::env::consts::OS,
                                arch = std::env::consts::ARCH
                            )),
                    ),
            )
            .child(
                small_button("about-copy-version", &tr!("Copy version info"))
                    .debug_selector(|| "about-copy-version".into())
                    .on_click(cx.listener(|v, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(version_info()));
                        v.say(tr!("Copied: {info}", info = version_info()), cx);
                    })),
            )
    }

    /// Which channel this build is on, what a look found, and getting it.
    fn updates(&self, cx: &mut Context<Self>) -> gpui::Div {
        let u = &self.snap.update;
        let on = u.check;
        let status: gpui::Div = if !u.keys {
            label(&tr!(
                "This build has no release keys, so it cannot tell of updates."
            ))
        } else if let UpdateStage::Ready(v) = &u.stage {
            update_card(
                tr!("shellwing {version} is ready", version = v.to_string()),
                div().child(label(&tr!(
                    "It is installed when you quit, or now with Restart."
                ))),
            )
            .child(
                small_button("update-restart", &tr!("Restart now"))
                    .on_click(cx.listener(|v, _, w, cx| v.run(Action::RestartToUpdate, w, cx))),
            )
        } else if let UpdateStage::Downloading(got, of) = u.stage {
            let v = u.offered.clone().unwrap_or_default();
            update_card(
                tr!("shellwing {version}", version = v),
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(label(&Workspace::download_said(got, of)))
                    .when_some(of.filter(|n| *n > 0), |d, of| {
                        let done = (got as f32 / of as f32).clamp(0., 1.);
                        d.child(
                            div()
                                .h(px(4.))
                                .w_full()
                                .rounded_full()
                                .bg(rgb(colors().edge))
                                .child(
                                    div()
                                        .h_full()
                                        .w(gpui::relative(done))
                                        .rounded_full()
                                        .bg(rgb(colors().accent)),
                                ),
                        )
                    }),
            )
        } else if u.checking {
            label(&tr!("Looking for a newer version…"))
        } else if let Some(v) = &u.offered {
            let get = match u.managed {
                Some(_) => tr!("How to update"),
                None if u.updates_itself => match u.stage {
                    UpdateStage::Failed(_) => tr!("Try again"),
                    _ => tr!("Download and install"),
                },
                None => tr!("Download"),
            };
            let said = match u.managed {
                Some((by, Some(command))) => {
                    tr!(
                        "{by} updates it: {command}",
                        by = crate::workspace::by_said(by),
                        command = command
                    )
                }
                Some((by, None)) => {
                    tr!("{by} updates it.", by = crate::workspace::by_said(by))
                }
                None if u.updates_itself => {
                    tr!("It is downloaded and checked, then installed when you quit.")
                }
                None => tr!("Download it, and install it in place of this one."),
            };
            update_card(
                tr!("shellwing {version} is out", version = v.clone()),
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(label(&said))
                    .when_some(
                        match &u.stage {
                            UpdateStage::Failed(why) => Some(why.clone()),
                            _ => None,
                        },
                        |d, why| {
                            d.child(
                                div()
                                    .text_size(ui(12.))
                                    .text_color(rgb(colors().bad))
                                    .child(tr!("The download failed: {why}", why = why)),
                            )
                        },
                    ),
            )
            .child(
                small_button("update-get", &get)
                    .on_click(cx.listener(|v, _, w, cx| v.run(Action::Update, w, cx))),
            )
            .child(
                small_button("update-skip", &tr!("Skip this version")).on_click(cx.listener(
                    |v, _, _, cx| {
                        let _ = v.workspace.update(cx, |ws, cx| ws.skip_update(cx));
                    },
                )),
            )
        } else if let Some(p) = &u.problem {
            div()
                .text_size(ui(12.))
                .text_color(rgb(colors().bad))
                .child(tr!("The last look failed: {why}", why = p.clone()))
        } else if let Some(t) = u.checked_at {
            label(&tr!(
                "Up to date (looked {when}).",
                when = crate::sync_panel::ago(t)
            ))
        } else {
            label(&tr!("Not looked yet."))
        };
        section(
            &tr!("Updates"),
            card()
                .child(label(&match u.channel {
                    "beta" => tr!(
                        "This is a beta: you are told of newer betas, and of the release that follows them."
                    ),
                    _ => tr!("This is a release: you are told of newer releases, not of betas."),
                }))
                .child(status)
                .when(u.keys, |d| d.child(
                    div()
                        .flex()
                        .child(
                            small_button("update-check", &tr!("Check now"))
                                .when(u.checking, |d| d.opacity(0.5))
                                .on_click(cx.listener(|v, _, _, cx| {
                                    let _ = v.workspace.update(cx, |ws, cx| ws.check_updates(cx));
                                })),
                        ),
                ))
                .when(u.keys, |d| d.child(toggle(
                    "update-auto",
                    on,
                    &tr!("Look for updates once a day"),
                    &tr!("It reads the list of releases from GitHub, and believes it only when its signatures verify. Nothing is downloaded or installed without you."),
                    cx.listener(move |v, _, _, cx| {
                        v.change(&["updates", "check"], Some(SettingValue::Bool(!on)), cx)
                    }),
                ))),
        )
    }
}

/// The app's icon as the window draws it: the wing on a pale tile.
fn app_logo() -> gpui::Div {
    div()
        .size(px(64.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(15.))
        .border_2()
        .border_color(rgb(0xb9c3cf))
        .bg(rgb(0xeef1f5))
        .child(
            svg()
                .path("icons/logo.svg")
                .size(px(44.))
                .text_color(rgb(0x0284c7)),
        )
}

/// HivePaaS's mark, seven cells of a hive in its brown and amber, on a pale
/// tile like shellwing's (the brown does not show on a dark one).
fn hivepaas_logo() -> gpui::Div {
    const BROWN: u32 = 0x543d2b;
    const AMBER: u32 = 0xf59e0b;
    // Centers in the mark's 36-unit box.
    const CELLS: [(f32, f32, u32); 7] = [
        (12.98, 9.30, BROWN),
        (23.02, 9.30, AMBER),
        (7.95, 18.0, BROWN),
        (18.0, 18.0, AMBER),
        (28.05, 18.0, AMBER),
        (12.98, 26.70, BROWN),
        (23.02, 26.70, AMBER),
    ];
    let mark = div()
        .relative()
        .size(px(36.))
        .children(CELLS.map(|(x, y, c)| {
            div()
                .absolute()
                .left(px(x - 4.3))
                .top(px(y - 4.3))
                .size(px(8.6))
                .rounded_full()
                .bg(rgb(c))
        }));
    div()
        .size(px(52.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(12.))
        .border_1()
        .border_color(rgb(0xd9cbb5))
        .bg(rgb(0xfbf7f0))
        .child(mark)
}

/// A release, as About shows it: what it is, and what is said of it; the
/// buttons follow.
fn update_card(title: String, body: gpui::Div) -> gpui::Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(10.))
        .p(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(colors().accent))
        .bg(rgb(colors().selected))
        .child(
            div()
                .flex_1()
                .min_w(px(220.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(body),
        )
}

/// A box on the page holding a section's content.
fn card() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .p(px(16.))
        .rounded(px(10.))
        .border_1()
        .border_color(rgb(colors().line))
        .bg(rgb(colors().surface))
}

fn section(title: &str, body: gpui::Div) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(heading(title))
        .child(body)
}

fn body_text(s: String) -> gpui::Div {
    div()
        .text_size(ui(13.))
        .text_color(rgb(colors().soft))
        .child(s)
}

/// A button that opens a web page.
fn link_button(
    id: &'static str,
    text: &str,
    url: String,
    cx: &mut Context<SettingsView>,
) -> gpui::Stateful<gpui::Div> {
    small_button(id, text)
        .debug_selector(|| id.into())
        .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))
}

/// A brand button's fill, lighter at the top, the same in either mode:
/// shellwing's is the logo's blue, HivePaaS's amber.
fn brand_fill(top: u32, bottom: u32) -> gpui::Background {
    gpui::linear_gradient(
        180.,
        gpui::linear_color_stop(rgb(top), 0.),
        gpui::linear_color_stop(rgb(bottom), 1.),
    )
}

/// A filled button that opens a web page: the section's main way on.
fn primary_link(
    id: &'static str,
    text: &str,
    url: &'static str,
    (bg, hover, fg): (gpui::Background, gpui::Background, u32),
    cx: &mut Context<SettingsView>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(|| id.into())
        .h(px(28.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .bg(bg)
        .text_size(ui(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(fg))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(text.to_string())
        .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url)))
}

/// Where the code is, under which license, and the ways to take part.
fn open_source(cx: &mut Context<SettingsView>) -> gpui::Div {
    let actions = div()
        .flex()
        .flex_wrap()
        .gap(px(8.))
        .child(primary_link(
            "about-site",
            &tr!("Visit shellwing.app"),
            SITE,
            (
                brand_fill(0x38b6f3, 0x0f87c9),
                brand_fill(0x5cc6f7, 0x1a95d6),
                0xffffff,
            ),
            cx,
        ))
        .child(link_button("about-github", "GitHub", REPO.to_string(), cx))
        .child(link_button(
            "about-issue",
            &tr!("Report a problem"),
            repo("issues/new"),
            cx,
        ))
        .child(link_button(
            "about-releases",
            &tr!("Release notes"),
            repo("releases"),
            cx,
        ));
    section(
        &tr!("Open source"),
        card()
            .child(body_text(tr!(
                "shellwing is free and open source, under the GPL-3.0-or-later license. Read the code, build it yourself, report a problem or send a fix."
            )))
            .child(actions),
    )
}

/// HivePaaS: what it is, where it runs, and where to see more.
fn hivepaas(cx: &mut Context<SettingsView>) -> gpui::Div {
    let c = colors();
    let title = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(14.))
        .child(hivepaas_logo())
        .child(
            div()
                .flex_1()
                .min_w(px(220.))
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(ui(17.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("HivePaaS"),
                        )
                        .child(
                            div()
                                .px(px(8.))
                                .rounded_full()
                                .border_1()
                                .border_color(rgb(c.warn_edge))
                                .bg(rgb(c.warn_bg))
                                .text_size(ui(11.))
                                .text_color(rgb(c.warn_text))
                                .child(tr!("Open source · Apache-2.0")),
                        ),
                )
                .child(
                    div()
                        .text_size(ui(12.5))
                        .text_color(rgb(c.muted))
                        .child(tr!(
                            "A self-hosted platform for your apps, built on Docker Swarm"
                        )),
                ),
        );
    let place = |icon: &'static str, name: String, what: String| {
        div()
            .flex_1()
            .min_w(px(240.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .py(px(12.))
            .px(px(14.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(c.line))
            .bg(rgb(c.page))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(svg().path(icon).size(px(16.)).text_color(rgb(c.amber)))
                    .child(name),
            )
            .child(
                div()
                    .text_size(ui(12.5))
                    .text_color(rgb(c.soft))
                    .child(what),
            )
    };
    let places = div()
        .flex()
        .flex_wrap()
        .gap(px(12.))
        .child(place(
            "icons/server.svg",
            tr!("On your servers"),
            tr!(
                "Deploy from Git or a Docker image, with domains and automatic TLS, logs, metrics and scaling across a cluster."
            ),
        ))
        .child(place(
            "icons/laptop.svg",
            tr!("On your own computer"),
            tr!(
                "Install it on your own computer in place of Docker Desktop or Portainer, and still get full app management on top."
            ),
        ));
    let features = [
        tr!("300+ one-click apps"),
        tr!("Deploy on push"),
        tr!("Pull request previews"),
        tr!("Automatic TLS"),
        tr!("Live logs and metrics"),
        tr!("Functions"),
    ];
    let chips = div()
        .flex()
        .flex_wrap()
        .gap(px(6.))
        .children(features.map(|f| {
            div()
                .px(px(10.))
                .py(px(2.))
                .rounded_full()
                .bg(rgb(c.chip))
                .text_size(ui(12.))
                .text_color(rgb(c.soft))
                .child(f)
        }));
    let actions = div()
        .flex()
        .flex_wrap()
        .gap(px(8.))
        .child(primary_link(
            "about-hivepaas",
            &tr!("Visit hivepaas.com"),
            HIVEPAAS,
            (
                brand_fill(0xfbbd2a, 0xf49a12),
                brand_fill(0xfcca4d, 0xf6a823),
                0x0b0c10,
            ),
            cx,
        ))
        .child(link_button(
            "about-hivepaas-github",
            "GitHub",
            HIVEPAAS_REPO.to_string(),
            cx,
        ));
    section(
        &tr!("From the same author"),
        card()
            .gap(px(16.))
            .child(title)
            .child(
                div()
                    .text_size(ui(14.))
                    .text_color(rgb(c.text))
                    .child(tr!(
                        "You might also like HivePaaS, from the same author: an open-source, self-hosted alternative to Heroku, Render and Coolify."
                    )),
            )
            .child(places)
            .child(chips)
            .child(actions),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_info_names_the_build() {
        let info = version_info();
        assert!(info.starts_with(&format!("shellwing {} (", env!("CARGO_PKG_VERSION"))));
        assert!(info.ends_with(&format!("{})", std::env::consts::ARCH)));
    }

    #[test]
    fn links_stay_in_the_repository() {
        assert_eq!(
            repo("issues/new"),
            "https://github.com/hivepaas/shellwing/issues/new"
        );
    }
}
