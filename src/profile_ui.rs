//! Native profile/import/library sheet. Data work runs off the UI executor.
use frame_profile::{
    LibraryEntry, ProfileStore,
    credentials::Keychain,
    migration::{self, Categories, SourceProfile},
};
use gpui_kit::base::Selectable as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    input::{Input, InputState},
    spinner::Spinner,
};
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, Global, InteractiveElement as _, IntoElement,
    ParentElement as _, PathPromptOptions, Render, StatefulInteractiveElement as _, Styled as _,
    StyledImage as _, Window, div, px,
};

#[derive(Clone)]
pub struct ActiveProfile(pub ProfileStore);
impl Global for ActiveProfile {}

pub enum ProfileEvent {
    Open(url::Url),
}
#[derive(Clone, Copy, PartialEq)]
pub enum Section {
    Import,
    Profiles,
    Bookmarks,
    History,
    Trail,
    Extensions,
}

pub struct ProfilePanel {
    store: ProfileStore,
    section: Section,
    sources: Vec<SourceProfile>,
    selected: Option<usize>,
    categories: Categories,
    busy: bool,
    message: String,
    imported: Option<ProfileStore>,
    name: Entity<InputState>,
    filter: Entity<InputState>,
    entries: Vec<LibraryEntry>,
    visits: Vec<(
        frame_profile::Visit,
        Option<std::sync::Arc<gpui_kit::RenderImage>>,
    )>,
    trail_before: Option<i64>,
    trail_task: Option<gpui_kit::Task<()>>,
    profiles: Vec<frame_profile::Profile>,
    extensions: Vec<(String, String)>,
    _filter_events: gpui_kit::Subscription,
}
impl EventEmitter<ProfileEvent> for ProfilePanel {}
impl ProfilePanel {
    pub fn new(
        store: ProfileStore,
        section: Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("New profile name"));
        let filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Find a title or address"));
        let events = cx.subscribe(&filter, |this, _, event, cx| {
            if matches!(event, gpui_kit::component::input::InputEvent::Change) {
                this.refresh(cx);
            }
        });
        let mut panel = Self {
            store,
            section,
            sources: vec![],
            selected: None,
            categories: Categories::default(),
            busy: false,
            message: String::new(),
            imported: None,
            name,
            filter,
            entries: vec![],
            visits: vec![],
            trail_before: None,
            trail_task: None,
            profiles: vec![],
            extensions: vec![],
            _filter_events: events,
        };
        panel.refresh(cx);
        if section == Section::Import {
            panel.discover(cx);
        }
        panel
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.section == Section::Trail {
            self.load_trail(cx);
            return;
        }
        self.profiles = self.store.profiles().unwrap_or_default();
        if self.section == Section::Extensions {
            self.extensions = (|| -> anyhow::Result<Vec<(String, String)>> {
                let db = self.store.connect()?;
                Ok(db
                    .prepare("SELECT name,version FROM extensions ORDER BY name")?
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<_, _>>()?)
            })()
            .unwrap_or_default();
        }
        if matches!(self.section, Section::Bookmarks | Section::History) {
            match self.store.suggestions(
                &self.filter.read(cx).value(),
                self.section == Section::Bookmarks,
                100,
            ) {
                Ok(entries) => self.entries = entries,
                Err(_) => self.message = "Could not read this profile's library".into(),
            }
        }
        cx.notify();
    }
    fn load_trail(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        let store = self.store.clone();
        let before = self.trail_before;
        self.trail_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    store.visits(before, 50).map(|visits| {
                        visits
                            .into_iter()
                            .map(|visit| {
                                let image = visit
                                    .preview
                                    .as_ref()
                                    .and_then(|bytes| image::load_from_memory(bytes).ok())
                                    .map(|image| {
                                        let mut rgba = image.to_rgba8();
                                        for pixel in rgba.chunks_exact_mut(4) {
                                            pixel.swap(0, 2);
                                        }
                                        std::sync::Arc::new(gpui_kit::RenderImage::new(vec![
                                            image::Frame::new(rgba),
                                        ]))
                                    });
                                (visit, image)
                            })
                            .collect()
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(visits) => {
                        this.visits = visits;
                        this.message.clear();
                    }
                    Err(error) => this.message = format!("Could not read the visit trail: {error}"),
                }
                cx.notify();
            });
        }));
    }
    fn discover(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        let task = cx.background_spawn(async { migration::discover() });
        cx.spawn(async move |this, cx| {
            let sources = task.await;
            let _ = this.update(cx, |this, cx| {
                this.sources = sources;
                this.busy = false;
                cx.notify();
            });
        })
        .detach();
    }
    fn import(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.selected.and_then(|i| self.sources.get(i)).cloned() else {
            return;
        };
        self.busy = true;
        self.message = "Importing selected data…".into();
        let store = self.store.clone();
        let categories = self.categories;
        let task = cx.background_spawn(async move {
            let target = store.imported_profile(&source.identity(), &source.name, &source.email)?;
            let report = migration::import(
                &target,
                &source,
                categories,
                &Keychain::for_profile(&target),
            );
            Ok::<_, anyhow::Error>((target, report))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ =
                this.update(cx, |this, cx| {
                    match result {
                        Ok((target, report)) => {
                            this.message = report.summary();
                            this.imported = Some(target);
                        }
                        Err(_) => this.message =
                            "Could not create the destination profile. No source data was changed."
                                .into(),
                    }
                    this.busy = false;
                    cx.notify();
                });
        })
        .detach();
    }
    fn import_file(&mut self, passwords: bool, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                if passwords {
                    "Import password CSV"
                } else {
                    "Import bookmarks HTML"
                }
                .into(),
            ),
        });
        let store = self.store.clone();
        self.busy = true;
        cx.spawn(async move |this, cx| {
            let selected = picker.await;
            let path = match selected {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = path else {
                let _ = this.update(cx, |this, cx| {
                    this.busy = false;
                    cx.notify();
                });
                return;
            };
            let task = cx.background_spawn(async move {
                if passwords {
                    migration::password_csv(&store, &path, &Keychain::for_profile(&store))
                } else {
                    migration::bookmarks_html(&store, &path)
                }
            });
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.message = match result {
                    Ok(c) => format!(
                        "{} imported, {} skipped. {}",
                        c.imported,
                        c.skipped,
                        if passwords {
                            "Passwords are in Keychain. You can remove your exported CSV."
                        } else {
                            "Bookmarks are in the current profile."
                        }
                    ),
                    Err(e) => format!("{e:#}"),
                };
                this.busy = false;
                this.refresh(cx);
            });
        })
        .detach();
    }
    fn select_profile(&mut self, id: i64, cx: &mut Context<Self>) {
        match self.store.select(id) {
            Ok(()) => cx.restart(),
            Err(_) => {
                self.message = "Could not switch profiles".into();
                cx.notify();
            }
        }
    }
}
impl Render for ProfilePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let mut content = v_flex().gap_3().w_full();
        let mut sections = h_flex().gap_1().flex_wrap();
        for (ix, (section, label)) in [
            (Section::Import, "Import"),
            (Section::Profiles, "Profiles"),
            (Section::Bookmarks, "Bookmarks"),
            (Section::History, "History"),
            (Section::Trail, "Visit trail"),
            (Section::Extensions, "Extensions"),
        ]
        .into_iter()
        .enumerate()
        {
            sections = sections.child(
                Button::new(("section", ix))
                    .label(label)
                    .small()
                    .selected(self.section == section)
                    .disabled(self.busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.section = section;
                        this.refresh(cx);
                        if section == Section::Import && this.sources.is_empty() {
                            this.discover(cx);
                        }
                    })),
            );
        }
        content = content.child(sections).child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(format!(
                    "Current profile: {} {}",
                    self.store.profile.name, self.store.profile.email
                )),
        );
        match self.section {
            Section::Import => {
                content=content.child(div().text_lg().child("Bring your browser data"))
                    .child(div().text_sm().child("Choose a local profile and the data to import. Each source gets a separate profile. No browser account is required."));
                if self.sources.is_empty() && !self.busy {
                    content=content.child("No Dia, Firefox or Chrome-family profiles found. You can still import exported files below.");
                }
                for (ix, source) in self.sources.iter().enumerate() {
                    content = content.child(
                        div()
                            .w_full()
                            .debug_selector(move || format!("import-source-{ix}"))
                            .child(
                                Button::new(("source", ix))
                                    .w_full()
                                    .label(format!(
                                        "{} · {} {}",
                                        source.browser, source.name, source.email
                                    ))
                                    .selected(self.selected == Some(ix))
                                    .disabled(self.busy)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.selected = Some(ix);
                                        this.message.clear();
                                        this.imported = None;
                                        cx.notify();
                                    })),
                            ),
                    );
                }
                content=content.child(Checkbox::new("import-bookmarks").label("Bookmarks and folders").checked(self.categories.bookmarks).disabled(self.busy)
                    .on_change(cx.listener(|this,v,_,cx|{this.categories.bookmarks=*v;cx.notify();})))
                    .child(Checkbox::new("import-history").label("History").checked(self.categories.history).disabled(self.busy)
                    .on_change(cx.listener(|this,v,_,cx|{this.categories.history=*v;cx.notify();})))
                    .child(Checkbox::new("import-passwords").label("Saved passwords · macOS Keychain storage").checked(self.categories.passwords).disabled(self.busy)
                    .on_change(cx.listener(|this,v,_,cx|{this.categories.passwords=*v;cx.notify();})))
                    .child(Checkbox::new("import-extensions").label("Extension inventory").checked(self.categories.extensions).disabled(self.busy)
                    .on_change(cx.listener(|this,v,_,cx|{this.categories.extensions=*v;cx.notify();})))
                    .child(Checkbox::new("import-tabs").label("Open tabs · supported sessions, combined into one window")
                        .checked(self.categories.tabs).disabled(self.busy)
                        .on_change(cx.listener(|this,v,_,cx|{this.categories.tabs=*v;cx.notify();})))
                    .child(Checkbox::new("import-forms").label("Names, email and basic form information")
                        .checked(self.categories.forms).disabled(self.busy)
                        .on_change(cx.listener(|this,v,_,cx|{this.categories.forms=*v;cx.notify();})))
                    .child(h_flex().justify_end().debug_selector(||"import-profile-action".into()).child(Button::new("import-profile").label("Import selected data").primary()
                        .disabled(self.busy||self.selected.is_none()|| !(self.categories.bookmarks||self.categories.history||self.categories.passwords||self.categories.extensions||self.categories.tabs||self.categories.forms))
                        .on_click(cx.listener(|this,_,_,cx|this.import(cx)))))
                    .child(div().text_sm().text_color(theme.muted_foreground).child("Account names transfer as labels. Websites may ask you to sign in again. Cookies, passkeys and extension execution are not transferred."));
                if let Some(target) = &self.imported {
                    let id = target.profile.id;
                    content = content.child(
                        Button::new("open-imported")
                            .label(format!("Open {} and restart", target.profile.name))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.select_profile(id, cx)),
                            ),
                    );
                }
                content = content
                    .child(
                        div()
                            .mt_3()
                            .child(format!("Import a file into {}", self.store.profile.name)),
                    )
                    .child(
                        Button::new("password-csv")
                            .label("Password CSV…")
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.import_file(true, cx))),
                    )
                    .child(
                        Button::new("bookmarks-html")
                            .label("Bookmarks HTML…")
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.import_file(false, cx))),
                    );
            }
            Section::Profiles => {
                content=content.child("Profiles keep history, credentials, search data and website storage separate. Switching restarts the browser.");
                {
                    for profile in &self.profiles {
                        let id = profile.id;
                        content = content.child(
                            Button::new(("profile", id as u64))
                                .label(format!("{} {}", profile.name, profile.email))
                                .disabled(id == self.store.profile.id || self.busy)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.select_profile(id, cx)),
                                ),
                        );
                    }
                }
                content = content.child(Input::new(&self.name)).child(
                    Button::new("create-profile")
                        .label("Create profile")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let name = this.name.read(cx).value().trim().to_owned();
                            if name.is_empty() {
                                this.message = "Enter a profile name".into();
                            } else {
                                match this.store.imported_profile(
                                    &format!("manual:{}", frame_profile::now()),
                                    &name,
                                    "",
                                ) {
                                    Ok(_) => {
                                        this.message =
                                            "Profile created. Select it above to open it.".into()
                                    }
                                    Err(_) => this.message = "Could not create profile".into(),
                                }
                            }
                            this.refresh(cx);
                        })),
                );
            }
            Section::Bookmarks | Section::History => {
                content = content.child(Input::new(&self.filter));
                if self.entries.is_empty() {
                    content = content.child(
                        "No matching entries. Import browser data or visit a page to get started.",
                    );
                }
                for (ix, entry) in self.entries.iter().enumerate() {
                    let url = entry.url.clone();
                    content = content
                        .child(
                            Button::new(("entry", ix))
                                .w_full()
                                .label(if entry.title.is_empty() {
                                    entry.url.clone()
                                } else {
                                    entry.title.clone()
                                })
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    if let Ok(url) = url::Url::parse(&url) {
                                        cx.emit(ProfileEvent::Open(url));
                                    }
                                })),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("{} {}", entry.folder, entry.url)),
                        );
                }
                if self.entries.len() == 100 {
                    content = content.child("Showing 100 entries. Type to narrow your search.");
                }
            }
            Section::Trail => {
                content = content.child("Each card is a separate visit. Reopening loads the live address; the preview remains the captured visit.");
                if self.visits.is_empty() && !self.busy {
                    content = content.child("No visits on this page of the trail.");
                }
                for (visit, preview) in &self.visits {
                    let url = visit.url.clone();
                    let date = chrono::DateTime::from_timestamp_micros(visit.visited)
                        .map(|t| {
                            t.with_timezone(&chrono::Local)
                                .format("%b %d · %H:%M:%S")
                                .to_string()
                        })
                        .unwrap_or_default();
                    let mut row = v_flex().gap_1().p_2().rounded_md().bg(theme.secondary);
                    if let Some(preview) = preview {
                        row = row.child(
                            gpui_kit::img(preview.clone())
                                .w_full()
                                .h(px(160.))
                                .object_fit(gpui_kit::ObjectFit::Contain),
                        );
                    }
                    row = row
                        .child(
                            Button::new(gpui_kit::SharedString::from(format!(
                                "visit:{}",
                                visit.id
                            )))
                            .label(if visit.title.is_empty() {
                                visit.url.clone()
                            } else {
                                visit.title.clone()
                            })
                            .on_click(cx.listener(
                                move |_, _, _, cx| {
                                    if let Ok(url) = url::Url::parse(&url) {
                                        cx.emit(ProfileEvent::Open(url));
                                    }
                                },
                            )),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("{date} · {}", visit.url)),
                        );
                    content = content.child(row);
                }
                let before = self.visits.last().map(|(v, _)| v.id);
                content = content.child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("trail-newest")
                                .label("Latest visits")
                                .disabled(self.busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.trail_before = None;
                                    this.load_trail(cx);
                                })),
                        )
                        .child(
                            Button::new("trail-older")
                                .label("Older visits")
                                .disabled(self.busy || self.visits.len() < 50)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.trail_before = before;
                                    this.load_trail(cx);
                                })),
                        ),
                );
            }
            Section::Extensions => {
                content=content.child("Imported extension inventory. These extensions need a compatible Platinum port before they can run.");
                for (name, version) in &self.extensions {
                    content =
                        content.child(format!("{name} {version} · No compatible port installed"));
                }
            }
        }
        if self.busy {
            content = content.child(Spinner::new());
        }
        if !self.message.is_empty() {
            content = content.child(div().text_sm().child(self.message.clone()));
        }
        div()
            .id("profile-panel")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .child(content.pb_6())
            .min_w(px(240.))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::Root;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext, size};
    #[gpui_kit::test]
    fn selecting_a_source_and_importing_keeps_its_data_out_of_the_current_profile(
        cx: &mut TestAppContext,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let store = ProfileStore::open(temp.path()).unwrap();
        std::fs::write(source.path().join("Bookmarks"),r#"{"roots":{"bookmark_bar":{"type":"folder","name":"Work","children":[{"type":"url","url":"https://example.test/","name":"Work notes"}]}}}"#).unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let mut panel = None;
        let window = cx.open_window(size(px(600.), px(1000.)), |window, cx| {
            let entity = cx.new(|cx| {
                let mut panel = ProfilePanel::new(store.clone(), Section::Profiles, window, cx);
                panel.section = Section::Import;
                panel.categories = Categories {
                    history: false,
                    extensions: false,
                    ..Default::default()
                };
                panel.sources = vec![SourceProfile {
                    format: frame_profile::migration::SourceFormat::Chromium,
                    browser: "Chrome".into(),
                    name: "Work".into(),
                    email: "work@example.test".into(),
                    directory: source.path().into(),
                    keychain_service: String::new(),
                    keychain_account: String::new(),
                }];
                panel
            });
            panel = Some(entity.clone());
            Root::new(entity, window, cx)
        });
        let panel = panel.unwrap();
        let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        fn center(
            cx: &mut VisualTestContext,
            name: &'static str,
        ) -> gpui_kit::Point<gpui_kit::Pixels> {
            cx.debug_bounds(name).expect("rendered control").center()
        }
        let import = cx
            .debug_bounds("import-profile-action")
            .map(|b| gpui_kit::point(b.right() - px(32.), b.center().y))
            .expect("import button row");
        cx.simulate_click(import, Modifiers::none());
        cx.run_until_parked();
        assert!(cx.update(|_, cx| panel.read(cx).imported.is_none()));
        let source = center(cx, "import-source-0");
        cx.simulate_click(source, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| panel.read(cx).selected), Some(0));
        let import = cx
            .debug_bounds("import-profile-action")
            .map(|b| gpui_kit::point(b.right() - px(32.), b.center().y))
            .expect("import button row");
        cx.simulate_click(import, Modifiers::none());
        cx.run_until_parked();
        let imported = cx
            .update(|_, cx| panel.read(cx).imported.clone())
            .expect("import completed");
        assert_eq!(imported.profile.name, "Work");
        assert_eq!(imported.profile.email, "work@example.test");
        assert_eq!(
            imported.suggestions("Work notes", true, 10).unwrap().len(),
            1
        );
        assert!(
            store
                .suggestions("Work notes", true, 10)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            ProfileStore::open(temp.path()).unwrap().profile.id,
            store.profile.id
        );
    }
}
