//! Config files from the vault placed on hosts, from the palette or the
//! pane's menu: where (the path typed, the file's own offered), how long
//! (until the tab closes, or to stay), and what of a file there already.
//! Placed until the tab closes, they are taken away when it closes or the
//! app quits; those left on a host (the connection dropped, the app was
//! killed) are taken away the next time it connects, if unchanged.

use super::*;
use crate::placing::{self, Placed, There};
use crate::snippets_view::size_said;
use shellwing_config::PlacedFile;
use shellwing_vault::ConfigFile;
use zeroize::Zeroizing;

impl Workspace {
    /// The config files the palette offers to place on the focused pane's
    /// host, while it is connected (over SSH).
    pub(super) fn file_entries(&self, cx: &Context<Self>) -> Vec<Entry> {
        let unready = self.pane_unready(true, cx);
        let placed: Vec<String> = self
            .active_tab()
            .map(|v| {
                v.read(cx)
                    .placed_files()
                    .iter()
                    .map(|p| p.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let mut files: Vec<ConfigFile> = self
            .vault
            .ready()
            .and_then(|v| v.config_files().ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|f| self.fits_pane(&f.scope, cx))
            .collect();
        files.sort_by_key(|f| (f.category.trim().to_lowercase(), f.name.to_lowercase()));
        files
            .into_iter()
            .map(|f| {
                let category = f.category.trim();
                Entry::new(
                    Kind::File(f.id.clone()),
                    if category.is_empty() {
                        tr!("Config files")
                    } else {
                        tr!("Config files · {category}", category = category)
                    },
                    f.name.clone(),
                )
                .detail(format!("{} · {}", f.path, size_said(f.size)))
                .hint(if let Some(why) = &unready {
                    why.clone()
                } else if placed.contains(&f.id) {
                    tr!("placed: Enter places it again")
                } else {
                    "Enter".into()
                })
                .section(Part::ConfigFiles)
            })
            .collect()
    }

    fn config_file(&self, id: &str) -> Option<ConfigFile> {
        self.vault
            .ready()?
            .config_files()
            .ok()?
            .into_iter()
            .find(|f| f.id == id)
    }

    fn file_content(&self, id: &str) -> Result<Zeroizing<Vec<u8>>, String> {
        self.vault
            .ready()
            .ok_or_else(|| tr!("The vault is not open."))?
            .config_file_content(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| tr!("It is not in the vault any more."))
    }

    /// Asks where on the focused pane's host to place config file `id`.
    pub(super) fn ask_place(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(why) = self.pane_unready(true, cx) {
            return self.say(why, CONFIG_NOTICE, cx);
        }
        let (Some(file), Some(view)) = (self.config_file(id), self.active_tab().cloned()) else {
            return;
        };
        let title = view.read(cx).session().title.clone();
        let notes = vec![Entry::new(
            Kind::Note,
            "",
            tr!(
                "{name} · {size} · permissions {mode}",
                name = file.name.clone(),
                size = size_said(file.size),
                mode = format!("{:o}", file.mode)
            ),
        )];
        let path = file.path.clone();
        self.asking = Some(Asking::PlacePath(file));
        let palette =
            cx.new(|cx| Palette::ask_with(&tr!("Where on {host}", host = title), &path, notes, cx));
        self.show_palette(palette, false, window, cx);
    }

    /// The path typed: what is there now decides what is asked next.
    pub(super) fn answer_place_path(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Asking::PlacePath(file)) = self.asking.take() else {
            return;
        };
        self.close_palette(window, cx);
        let Some(view) = self.active_tab().cloned() else {
            return;
        };
        let content = match self.file_content(&file.id) {
            Ok(c) => c,
            Err(e) => return self.say(e, CONFIG_NOTICE, cx),
        };
        let remote = view.read(cx).session().remote();
        let Some(answer) = remote.sftp() else {
            return self.say(tr!("Not connected."), CONFIG_NOTICE, cx);
        };
        let task = remote.rt.spawn(async move {
            let sftp = placing::open(answer).await?;
            placing::look(&sftp, &path, &content).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let looked = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update_in(cx, |ws, window, cx| {
                ws.ask_place_how(file, view, looked, window, cx)
            });
        })
        .detach();
    }

    fn ask_place_how(
        &mut self,
        file: ConfigFile,
        view: Entity<TermView>,
        looked: Result<(String, There), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (full, there) = match looked {
            Ok(l) => l,
            Err(e) => {
                return self.say(
                    tr!("Cannot place {name}: {e}", name = file.name.clone(), e = e),
                    CONFIG_NOTICE,
                    cx,
                );
            }
        };
        let choice = |id: &str, title: String, detail: String| {
            Entry::new(Kind::Choice(id.into()), "", title).detail(detail)
        };
        // Placed here already until the tab closes: what was there before
        // stays set aside, to come back, unless it is kept there now.
        let ours = view.read(cx).placed_files().iter().any(|p| p.path == full);
        // The file's own way first, for Enter: to stay, or until the tab closes.
        let first = |stay: Vec<Entry>, until: Vec<Entry>| {
            let (mut a, b) = if file.stays {
                (stay, until)
            } else {
                (until, stay)
            };
            a.extend(b);
            a
        };
        let cancel = || choice("cancel", tr!("Cancel"), String::new());
        let mut entries = match there {
            _ if ours => {
                let again = (there != There::Same).then(|| {
                    choice(
                        "again",
                        tr!("Replace the one placed here until the tab closes"),
                        tr!("What was there before still comes back when the tab closes."),
                    )
                });
                let keep = choice(
                    "keep-placed",
                    tr!("Keep the one placed here"),
                    tr!("It stays when the tab closes."),
                );
                first(vec![keep], again.into_iter().collect())
            }
            There::Same => {
                return self.say(
                    tr!("{path} holds it already.", path = full.as_str()),
                    CONFIG_NOTICE,
                    cx,
                );
            }
            There::Nothing => first(
                vec![choice(
                    "keep",
                    tr!("Place it to stay"),
                    tr!("It stays when the tab closes."),
                )],
                vec![choice(
                    "until",
                    tr!("Place it until the tab closes"),
                    tr!("Then it is taken away."),
                )],
            ),
            There::Other => first(
                vec![
                    choice(
                        "bak",
                        tr!("Replace it, keeping the old one"),
                        tr!("As {path}", path = placing::backup_of(&full)),
                    ),
                    choice("over", tr!("Replace it"), tr!("The one there now is gone.")),
                ],
                vec![choice(
                    "until-over",
                    tr!("Replace the file there until the tab closes"),
                    tr!("The one there now is set aside, and comes back when the tab closes."),
                )],
            ),
        };
        if ours || there == There::Other {
            entries.push(cancel());
        }
        self.asking = Some(Asking::PlaceHow {
            file,
            full: full.clone(),
            view: view.downgrade(),
        });
        let title = match there {
            There::Other => tr!("{path} is there already…", path = full.as_str()),
            _ => tr!("Place at {path}…", path = full.as_str()),
        };
        let palette = cx.new(|cx| Palette::chooser(&title, entries, cx));
        self.show_palette(palette, false, window, cx);
    }

    /// How, chosen: places it.
    pub(super) fn chose_place(&mut self, how: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Asking::PlaceHow { file, full, view }) = self.asking.take() else {
            return;
        };
        self.close_palette(window, cx);
        if how == "keep-placed" {
            if let Some(view) = view.upgrade() {
                view.update(cx, |v, cx| v.keep_placed(Some(&full), cx));
            }
            return;
        }
        let before = view
            .upgrade()
            .and_then(|v| {
                v.read(cx)
                    .placed_files()
                    .iter()
                    .find(|p| p.path == full)
                    .map(|p| p.aside.clone())
            })
            .flatten();
        let (aside, until) = match how {
            "until" | "again" => (None, true),
            "keep" | "over" => (None, false),
            "until-over" => (Some(placing::aside_of(&full)), true),
            "bak" => (Some(placing::backup_of(&full)), false),
            _ => return,
        };
        let Some(view) = view.upgrade() else {
            return;
        };
        let content = match self.file_content(&file.id) {
            Ok(c) => c,
            Err(e) => return self.say(e, CONFIG_NOTICE, cx),
        };
        let remote = view.read(cx).session().remote();
        let Some(answer) = remote.sftp() else {
            return self.say(tr!("Not connected."), CONFIG_NOTICE, cx);
        };
        let placed = Placed {
            id: file.id.clone(),
            name: file.name.clone(),
            path: full.clone(),
            // Placed over one of ours: what that one set aside comes back.
            aside: if how == "again" {
                before
            } else {
                aside.clone().filter(|_| until)
            },
            sha256: placing::sha256_hex(&content),
        };
        let mode = file.mode;
        let task = remote.rt.spawn(async move {
            let sftp = placing::open(answer).await?;
            placing::place(&sftp, &full, &content, mode, aside.as_deref()).await
        });
        cx.spawn(async move |this, cx| {
            let done = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |ws, cx| match done {
                Ok(()) if until => {
                    let host = view.read(cx).session().spec.clone();
                    ws.keep_placed(placed.kept(&host));
                    let said = tr!(
                        "Placed {name} at {path} until the tab closes.",
                        name = placed.name.clone(),
                        path = placed.path.clone()
                    );
                    view.update(cx, |v, cx| {
                        v.add_placed(placed, cx);
                        v.show_notice(said, CONFIG_NOTICE, cx);
                    });
                }
                Ok(()) => {
                    let said = tr!(
                        "Placed {name} at {path}.",
                        name = placed.name.clone(),
                        path = placed.path.clone()
                    );
                    view.update(cx, |v, cx| v.show_notice(said, CONFIG_NOTICE, cx));
                }
                Err(e) => {
                    let said = tr!(
                        "Cannot place {name}: {e}",
                        name = placed.name.clone(),
                        e = e
                    );
                    view.update(cx, |v, cx| v.show_notice(said, CONFIG_NOTICE, cx));
                }
            });
        })
        .detach();
    }

    /// Notes a file placed until its tab closes, to take away later if the
    /// tab cannot.
    fn keep_placed(&self, p: PlacedFile) {
        let Some(store) = self.live.store() else {
            return;
        };
        let mut all = store.placed();
        all.retain(|x| !(x.host == p.host && x.path == p.path));
        all.push(p);
        if let Err(e) = store.save_placed(&all) {
            eprintln!("cannot note the placed files: {e}");
        }
    }

    /// Placed files on `host` gone (or left alone): no longer noted.
    pub(super) fn forget_placed(&self, host: &str, paths: &[String]) {
        let Some(store) = self.live.store() else {
            return;
        };
        let mut all = store.placed();
        let before = all.len();
        all.retain(|x| !(x.host == host && paths.contains(&x.path)));
        if all.len() != before
            && let Err(e) = store.save_placed(&all)
        {
            eprintln!("cannot note the placed files: {e}");
        }
    }

    /// `view` connected: files left on its host from before (not placed by
    /// a pane open now) are taken away if unchanged, else left as they are.
    pub(super) fn tidy_placed(&mut self, view: &Entity<TermView>, cx: &mut Context<Self>) {
        let Some(store) = self.live.store() else {
            return;
        };
        let spec = view.read(cx).session().spec.clone();
        let held: Vec<String> = self
            .tabs
            .iter()
            .flat_map(|t| t.views())
            .filter(|v| v.read(cx).session().spec == spec)
            .flat_map(|v| {
                v.read(cx)
                    .placed_files()
                    .iter()
                    .map(|p| p.path.clone())
                    .collect::<Vec<_>>()
            })
            .collect();
        let left: Vec<PlacedFile> = store
            .placed()
            .into_iter()
            .filter(|p| p.host == spec && !held.contains(&p.path))
            .collect();
        if left.is_empty() {
            return;
        }
        let remote = view.read(cx).session().remote();
        let Some(answer) = remote.sftp() else {
            return;
        };
        let task = remote.rt.spawn(async move {
            let sftp = placing::open(answer).await?;
            let (mut gone, mut kept) = (vec![], vec![]);
            for p in left {
                match placing::tidy(&sftp, &p).await {
                    Ok(true) => gone.push(p.path),
                    Ok(false) => kept.push(p.path),
                    Err(_) => {}
                }
            }
            Ok::<_, String>((gone, kept))
        });
        let view = view.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok((gone, kept))) = task.await else {
                return;
            };
            let _ = this.update(cx, |ws, cx| {
                ws.forget_placed(&spec, &gone);
                ws.forget_placed(&spec, &kept);
                let mut said = vec![];
                if !gone.is_empty() {
                    said.push(tr_n!(
                        gone.len(),
                        "Took away {n} config file left here from before.",
                        "Took away {n} config files left here from before."
                    ));
                }
                for p in &kept {
                    said.push(tr!(
                        "Left {path} as it is: it changed since it was placed.",
                        path = p.as_str()
                    ));
                }
                if !said.is_empty() {
                    view.update(cx, |v, cx| v.show_notice(said.join(" "), KEPT_NOTICE, cx));
                }
            });
        })
        .detach();
    }

    /// Asks for a file on the focused pane's host to keep in the vault.
    pub(super) fn ask_save_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.active_tab() else {
            return;
        };
        let title = view.read(cx).session().title.clone();
        let notes = vec![Entry::new(
            Kind::Note,
            "",
            tr!(
                "Kept encrypted as a config file, for this host, up to {max} KB.",
                max = shellwing_vault::limits::FILE / 1024
            ),
        )];
        self.asking = Some(Asking::SaveFile);
        let palette = cx.new(|cx| {
            Palette::ask_with(
                &tr!("A file on {host} to keep in the vault", host = title),
                "~/",
                notes,
                cx,
            )
        });
        self.show_palette(palette, false, window, cx);
    }

    pub(super) fn answer_save_file(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.asking = None;
        self.close_palette(window, cx);
        let Some(view) = self.active_tab().cloned() else {
            return;
        };
        let s = view.read(cx).session();
        let scope = s
            .entry
            .as_ref()
            .map(|e| snippets::Scope::Host(e.clone()).field())
            .unwrap_or_default();
        let remote = s.remote();
        let Some(answer) = remote.sftp() else {
            return self.say(tr!("Not connected."), CONFIG_NOTICE, cx);
        };
        let typed = path.clone();
        let task = remote.rt.spawn(async move {
            let sftp = placing::open(answer).await?;
            let full = sftp.expand(&typed).await.map_err(|e| e.to_string())?;
            sftp.read_file(&full, shellwing_vault::limits::FILE)
                .await
                .map_err(|e| e.to_string())?
                .map(Zeroizing::new)
                .ok_or_else(|| format!("nothing at {full}"))
        });
        cx.spawn(async move |this, cx| {
            let read = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |ws, cx| {
                let saved = read.and_then(|bytes| {
                    let name = shellwing_ssh::sftp::name_of(&path).to_string();
                    let f = ConfigFile {
                        id: String::new(),
                        name,
                        category: String::new(),
                        path: path.clone(),
                        mode: 0o600,
                        scope,
                        size: 0,
                        binary: false,
                        stays: false,
                    };
                    ws.vault
                        .ready()
                        .ok_or_else(|| tr!("The vault is not open."))?
                        .save_config_file(&f, Some(&bytes))
                        .map_err(|e| crate::environment::said_error(&e))
                });
                let said = match saved {
                    Ok(_) => {
                        ws.vault_changed();
                        tr!(
                            "Kept {path} in the vault: see Settings › Config files.",
                            path = path.as_str()
                        )
                    }
                    Err(e) => tr!("Not kept: {e}", e = e),
                };
                view.update(cx, |v, cx| v.show_notice(said, CONFIG_NOTICE, cx));
            });
        })
        .detach();
    }

    /// Takes away the files placed until their tab closes from `views`'
    /// hosts, then runs `then` (which closes them).
    pub(super) fn take_down_then(
        &mut self,
        views: Vec<Entity<TermView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let jobs: Vec<_> = views
            .iter()
            .filter_map(|v| {
                v.update(cx, |v, _| v.take_down(None))
                    .map(|j| (v.clone(), j))
            })
            .collect();
        if jobs.is_empty() {
            return then(self, window, cx);
        }
        self.say(tr!("Removing the config files placed…"), THEME_NOTICE, cx);
        cx.spawn_in(window, async move |this, cx| {
            let mut done = vec![];
            for (v, job) in jobs {
                let (gone, _) = job.await.unwrap_or_default();
                done.push((v, gone));
            }
            let _ = this.update_in(cx, |ws, window, cx| {
                for (v, gone) in done {
                    v.update(cx, |v, cx| v.took_down(&gone, cx));
                }
                then(ws, window, cx);
            });
        })
        .detach();
    }
}
