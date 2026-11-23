// SPDX-License-Identifier: MIT
//! EarthCraft in the browser, for Playground (patches/google-earth.yaml in github.com/daijro/openplayground): the
//! desktop window on the page's canvas, and its files in Playground's Files.
//!
//! `main` runs as on the desktop: the crate root takes [`eframe`] (and [`self_update`]) from here on the web, whose
//! `run_native` starts the app on the page with the desktop's own creator. Files live in memory
//! (earthcraft-playground's `std::fs`, where `HOME` is `/`): a file from Files comes in at its path
//! (`browser:/Places/trip.kml` is `/Places/trip.kml` here), and whatever EarthCraft writes outside its own folders (a
//! saved KML or planet, a screenshot in `/EarthCraft`) goes to Files after the frame. Open KML…, Save As…, the
//! Explorer's planet and image pickers are Playground's dialogs ([`rfd`]); "Open in EarthCraft" opens a KML/KMZ (or a
//! planet), as a file dropped on the globe does.
use super::App;
use ::eframe::egui;
use earthcraft_core::placemark::Places;
use earthcraft_playground::std::fs;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};

/// EarthCraft's own folders in memory, never copied to Files: settings and caches (`~/.cache`), scratch files.
const OWN: [&str; 2] = ["/tmp", "/.cache"];
/// Where dropped files are kept.
const DROPS: &str = "/tmp/EarthCraft dropped";
/// Where a file opened from the computer is kept.
const FROM_COMPUTER: &str = "/EarthCraft/Opened";

/// What a file dialog was for (see [`rfd`]).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Kml,
    Planet,
    /// The Explorer's planet texture, or its sky.
    Image {
        sky: bool,
    },
}

impl Kind {
    fn of(extensions: &[String], sky: bool) -> Self {
        if extensions.iter().any(|e| e.eq_ignore_ascii_case("kml")) {
            Kind::Kml
        } else if extensions.iter().any(|e| e == "earthplanet") {
            Kind::Planet
        } else {
            Kind::Image { sky }
        }
    }
    fn types(self) -> JsValue {
        let types: &[&str] = match self {
            Kind::Kml => &["kml", "kmz"],
            Kind::Planet => &["earthplanet"],
            Kind::Image { .. } => &["png", "jpg", "jpeg"],
        };
        types
            .iter()
            .map(|t| JsValue::from(*t))
            .collect::<js_sys::Array>()
            .into()
    }
}

/// A dialog the app asked for this frame.
#[derive(Clone, Debug)]
enum Ask {
    Open(Kind),
    Save(Kind, String),
}

#[derive(Default)]
pub(super) struct WorkspacePlaces {
    saved: Places,
    revision: Option<(PathBuf, String)>,
    saving: Option<(PathBuf, Places)>,
    dirty: bool,
}

thread_local! {
    static CTX: OnceCell<egui::Context> = const { OnceCell::new() };
    static READ_REVISIONS: RefCell<BTreeMap<PathBuf, String>> = const { RefCell::new(BTreeMap::new()) };
    static SAVE_RECEIPTS: RefCell<Vec<Result<String, String>>> = const { RefCell::new(Vec::new()) };
    static ASKED: RefCell<Option<Ask>> = const { RefCell::new(None) };
    /// The Explorer is drawing its sky row (explorer.rs says so on the web), so an image picked now is the sky.
    static SKY: Cell<bool> = const { Cell::new(false) };
    /// Files from Playground (Open, "Open in EarthCraft"), and what they were picked for, for the next frame.
    static OPENED: RefCell<Vec<(PathBuf, Option<Kind>)>> = const { RefCell::new(Vec::new()) };
    /// Where Save As… put a file, for the next frame.
    static SAVED: RefCell<Vec<(PathBuf, Kind)>> = const { RefCell::new(Vec::new()) };
    /// Dropped files whose bytes are in memory now, for the next frame.
    static DROPPED: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

fn repaint() {
    CTX.with(|c| c.get().map(egui::Context::request_repaint));
}

/// The Explorer's sky row is next: an image picked from here on, this frame, is the sky's.
pub(super) fn sky_section() {
    SKY.set(true);
}

/// The aircraft layer's checkbox on the web: off, saying why.
pub(super) fn no_flights(ui: &mut egui::Ui, label: &str) {
    ui.add_enabled(false, egui::Checkbox::new(&mut false, label)).on_disabled_hover_text(
        "Live aircraft are off in the browser: OpenSky and ADSB.lol don't let other sites' pages read their feeds.",
    );
}

/// The desktop's eframe, with `run_native` starting the app on the page instead.
pub(crate) mod eframe {
    pub use ::eframe::*;

    /// The desktop window's options, which a page has no use for.
    #[derive(Default)]
    #[allow(dead_code)]
    pub struct NativeOptions {
        pub viewport: egui::ViewportBuilder,
        pub wgpu_options: egui_wgpu::WgpuConfiguration,
    }

    /// Starts the app on the page's canvas, made by `creator` once the page's context exists; returns at once.
    pub fn run_native(
        _name: &str,
        _options: NativeOptions,
        creator: super::Creator,
    ) -> ::eframe::Result {
        super::start(creator)
    }
}

/// The desktop's updater restart, which a page never asks for (there are no updates to install here).
pub(crate) mod self_update {
    pub mod restart {
        pub fn restart() -> std::io::Result<std::convert::Infallible> {
            Err(std::io::Error::other("a page doesn't restart"))
        }
    }
}

/// The desktop's file dialogs: on the web they ask Playground's after the frame ([`Web::after`]) and give nothing now.
pub(crate) mod rfd {
    use std::path::{Path, PathBuf};

    #[derive(Default)]
    pub struct FileDialog {
        extensions: Vec<String>,
        name: Option<String>,
    }

    impl FileDialog {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn add_filter(
            mut self,
            _name: impl Into<String>,
            extensions: &[impl ToString],
        ) -> Self {
            self.extensions
                .extend(extensions.iter().map(ToString::to_string));
            self
        }
        pub fn set_title(self, _title: impl Into<String>) -> Self {
            self
        }
        pub fn set_file_name(mut self, name: impl Into<String>) -> Self {
            self.name = Some(name.into());
            self
        }
        pub fn set_directory(self, _dir: impl AsRef<Path>) -> Self {
            self
        }
        pub fn pick_file(self) -> Option<PathBuf> {
            let kind = super::Kind::of(&self.extensions, super::SKY.get());
            super::ASKED.set(Some(super::Ask::Open(kind)));
            None
        }
        pub fn save_file(self) -> Option<PathBuf> {
            let kind = super::Kind::of(&self.extensions, false);
            let name = self.name.unwrap_or_else(|| "placemarks.kml".into());
            super::ASKED.set(Some(super::Ask::Save(kind, name)));
            None
        }
        /// A page can't name a folder of the computer: the folder stays as typed.
        pub fn pick_folder(self) -> Option<PathBuf> {
            None
        }
    }
}

fn get(o: &JsValue, k: &str) -> JsValue {
    js_sys::Reflect::get(o, &k.into()).unwrap_or(JsValue::UNDEFINED)
}

/// `playgroundFiles[method](...args)`; None when the page has none or it threw.
fn call(method: &str, args: &[JsValue]) -> Option<JsValue> {
    let api = Some(get(&js_sys::global(), "playgroundFiles")).filter(JsValue::is_object)?;
    let f = get(&api, method).dyn_into::<js_sys::Function>().ok()?;
    f.apply(&api, &args.iter().collect::<js_sys::Array>()).ok()
}

async fn wait(v: Option<JsValue>) -> Option<JsValue> {
    let promise = v?.dyn_into::<js_sys::Promise>().ok()?;
    wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .ok()
        .filter(|v| !v.is_null() && !v.is_undefined())
}

fn options(pairs: &[(&str, JsValue)]) -> JsValue {
    let o = js_sys::Object::new();
    for (k, v) in pairs {
        let _ = js_sys::Reflect::set(&o, &(*k).into(), v);
    }
    o.into()
}

/// A file from Files (`{ path, name, bytes }`) into memory at its path; one from the computer goes to
/// `/EarthCraft/Opened`.
fn bring_in(file: &JsValue) -> Option<PathBuf> {
    let name = get(file, "name").as_string()?;
    let path = match get(file, "path")
        .as_string()
        .and_then(|p| p.strip_prefix("browser:").map(PathBuf::from))
    {
        Some(path) => path,
        None => Path::new(FROM_COMPUTER).join(Path::new(&name).file_name()?),
    };
    fs::put(&path, js_sys::Uint8Array::new(&get(file, "bytes")).to_vec());
    if let Some(revision) = get(file, "sha256").as_string() {
        READ_REVISIONS.with_borrow_mut(|revisions| revisions.insert(path.clone(), revision));
    }
    Some(path)
}

fn opened(file: JsValue, kind: Option<Kind>) {
    if let Some(path) = bring_in(&file) {
        OPENED.with_borrow_mut(|o| o.push((path, kind)));
        repaint();
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn update_binding(workspace: &WorkspacePlaces) {
    if let Some((path, sha)) = &workspace.revision {
        call(
            "bind",
            &[
                format!("browser:{}", path.display()).into(),
                sha.as_str().into(),
            ],
        );
    } else {
        call("bind", &[JsValue::NULL, JsValue::NULL]);
    }
}

fn report(app: &mut App, status: String) {
    call("status", &[status.as_str().into()]);
    app.status = status;
}

pub(super) fn kml_loaded(app: &mut App, path: &Path, first: bool) {
    app.workspace_places.revision = READ_REVISIONS
        .with_borrow_mut(|revisions| revisions.remove(path))
        .map(|sha| (path.to_path_buf(), sha));
    if first && app.workspace_places.revision.is_some() {
        app.workspace_places.saved = app.placemarks.clone();
    }
    update_binding(&app.workspace_places);
    call(
        "status",
        &[format!(
            "Opened {}",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("places")
        )
        .into()],
    );
}

pub(super) fn clear_kml_binding(app: &mut App) {
    app.workspace_places.revision = None;
    app.workspace_places.saved = Places::default();
    update_binding(&app.workspace_places);
}

pub(super) fn current_kml_path(app: &App) -> Option<PathBuf> {
    app.workspace_places
        .revision
        .as_ref()
        .map(|(path, _)| path.clone())
}

pub(super) fn saving_kml(app: &App) -> bool {
    app.workspace_places.saving.is_some()
}

pub(super) fn track_kml_save(app: &mut App, path: PathBuf) {
    app.workspace_places.saving = Some((path, app.placemarks.clone()));
}

pub(super) fn cancel_kml_save(app: &mut App) {
    app.workspace_places.saving = None;
}

pub(super) fn persist_kml(app: &mut App, path: PathBuf) {
    let Some((target, _)) = app.workspace_places.saving.as_mut() else {
        return;
    };
    *target = path.clone();
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            cancel_kml_save(app);
            report(
                app,
                format!("Cannot read the prepared places file: {error}"),
            );
            return;
        }
    };
    let expected = app
        .workspace_places
        .revision
        .as_ref()
        .filter(|(current, _)| current == &path)
        .map_or(JsValue::NULL, |(_, sha)| sha.as_str().into());
    let asked = call(
        "persist",
        &[
            format!("browser:{}", path.display()).into(),
            js_sys::Uint8Array::from(bytes.as_slice()).into(),
            expected,
        ],
    );
    wasm_bindgen_futures::spawn_local(async move {
        let result = match asked.and_then(|promise| promise.dyn_into::<js_sys::Promise>().ok()) {
            Some(promise) => wasm_bindgen_futures::JsFuture::from(promise)
                .await
                .map_err(|error| {
                    get(&error, "message")
                        .as_string()
                        .unwrap_or_else(|| "Files could not save the places.".into())
                })
                .and_then(|receipt| {
                    receipt
                        .as_string()
                        .ok_or_else(|| "Files did not confirm the places save.".into())
                }),
            None => Err("WebOS Files is unavailable.".into()),
        };
        SAVE_RECEIPTS.with_borrow_mut(|receipts| receipts.push(result));
        repaint();
    });
}

fn finish_workspace_save(app: &mut App, outcome: Result<String, String>) {
    let Some((path, saved)) = app.workspace_places.saving.take() else {
        return;
    };
    match outcome {
        Ok(revision) => {
            app.workspace_places.saved = saved;
            app.workspace_places.revision = Some((path.clone(), revision));
            update_binding(&app.workspace_places);
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("places");
            call("status", &[format!("Saved {name}").into()]);
            app.finish_kml_save(path);
        }
        Err(error) => report(app, error),
    }
}

/// The page's app: the window, then what it asked of the page.
pub(crate) struct Web {
    app: App,
}

impl Web {
    /// After the window's frame: files that came in, the dialogs it asked for, and what it wrote.
    fn after(&mut self) {
        SKY.set(false);
        for receipt in SAVE_RECEIPTS.take() {
            finish_workspace_save(&mut self.app, receipt);
        }
        for (path, kind) in OPENED.take() {
            let app = &mut self.app;
            match kind {
                Some(Kind::Image { sky }) => app
                    .explorer
                    .picked_image(path.to_string_lossy().into_owned(), sky),
                _ if extension(&path) == "earthplanet" => {
                    app.explorer.request_open(path, &app.file_jobs)
                }
                _ if app.kml_session.paths.contains(&path) => {
                    report(app, "That places file is already open.".into())
                }
                _ => app.load_kml_from(&path),
            }
        }
        for (path, kind) in SAVED.take() {
            match kind {
                Kind::Planet => self.app.explorer.picked_save(path, &self.app.file_jobs),
                _ => self.app.save_kml_to(path),
            }
        }
        match ASKED.take() {
            Some(Ask::Open(kind)) => {
                let asked = call("open", &[options(&[("types", kind.types())])]);
                wasm_bindgen_futures::spawn_local(async move {
                    if let Some(file) = wait(asked)
                        .await
                        .and_then(|p| js_sys::Array::from(&p).iter().next())
                    {
                        opened(file, Some(kind));
                    }
                });
            }
            Some(Ask::Save(kind, name)) => {
                let asked = call(
                    "saveAs",
                    &[options(&[
                        ("name", name.as_str().into()),
                        ("types", kind.types()),
                    ])],
                );
                wasm_bindgen_futures::spawn_local(async move {
                    let Some(answer) = wait(asked).await else {
                        return;
                    };
                    let path = get(&answer, "path")
                        .as_string()
                        .and_then(|path| path.strip_prefix("browser:").map(PathBuf::from));
                    if let Some(mut path) = path {
                        if path.extension().is_none() {
                            path.set_extension(if kind == Kind::Planet {
                                "earthplanet"
                            } else {
                                "kml"
                            });
                        }
                        SAVED.with_borrow_mut(|s| s.push((path, kind)));
                        repaint();
                    }
                });
            }
            None => {}
        }
        for p in fs::changes() {
            if matches!(extension(&p).as_str(), "kml" | "kmz")
                || OWN.iter().any(|own| p.starts_with(own))
            {
                continue;
            }
            let target = JsValue::from(format!("browser:{}", p.display()));
            match fs::metadata(&p) {
                Ok(m) if m.is_file() => {
                    call(
                        "write",
                        &[
                            target,
                            js_sys::Uint8Array::from(fs::read(&p).unwrap_or_default().as_slice())
                                .into(),
                        ],
                    );
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }
        let workspace = &mut self.app.workspace_places;
        let dirty = workspace.saving.is_some() || self.app.placemarks != workspace.saved;
        if dirty != workspace.dirty {
            workspace.dirty = dirty;
            call("setUnsaved", &[dirty.into()]);
        }
    }
}

/// A dropped file, in memory.
#[derive(Debug)]
struct InMemory(PathBuf);

impl egui::DroppedFile for InMemory {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes_async(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, String>> + '_>> {
        let bytes = fs::read(&self.0).map_err(|e| e.to_string());
        Box::pin(async move { bytes })
    }
}

impl ::eframe::App for Web {
    fn raw_input_hook(&mut self, _: &egui::Context, raw_input: &mut egui::RawInput) {
        // A dropped file is only a name on the web: once its bytes are in memory under DROPS, a later frame drops
        // that file instead, as the desktop window gets it.
        raw_input.dropped_files.extend(
            DROPPED
                .take()
                .into_iter()
                .map(|p| std::sync::Arc::new(InMemory(p)) as _),
        );
        for f in std::mem::take(&mut raw_input.dropped_files) {
            if f.path().parent() == Some(Path::new(DROPS)) {
                raw_input.dropped_files.push(f);
                continue;
            }
            wasm_bindgen_futures::spawn_local(async move {
                if let (Ok(bytes), Some(name)) = (f.bytes_async().await, f.path().file_name()) {
                    let path = Path::new(DROPS).join(name);
                    fs::put(&path, bytes);
                    DROPPED.with_borrow_mut(|d| d.push(path));
                    repaint();
                }
            });
        }
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut ::eframe::Frame) {
        ::eframe::App::logic(&mut self.app, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut ::eframe::Frame) {
        ::eframe::App::ui(&mut self.app, ui, frame);
        self.after();
        // A link (a source's credit, Releases) opens in a new tab, as the desktop opens the browser, rather than
        // replacing the app.
        ui.ctx().output_mut(|o| {
            for command in &mut o.commands {
                if let egui::OutputCommand::OpenUrl(url) = command {
                    url.new_tab = true;
                }
            }
        });
    }

    fn on_exit(&mut self) {
        ::eframe::App::on_exit(&mut self.app);
    }
}

/// The desktop's app creator, as `main` hands it over (a `Box<App>`, not eframe's `dyn App`).
pub(crate) type Creator =
    Box<dyn FnOnce(&::eframe::CreationContext<'_>) -> Result<Box<App>, DynError>>;
type DynError = Box<dyn std::error::Error + Send + Sync>;

/// [`eframe::run_native`] on the web: the app is made by the desktop's creator against the page's context, so the
/// services it starts wake the page.
fn start(creator: Creator) -> ::eframe::Result {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("EarthCraft: {info}").into())
    }));
    call(
        "onOpen",
        &[Closure::<dyn Fn(JsValue)>::new(|file| opened(file, None)).into_js_value()],
    );
    wasm_bindgen_futures::spawn_local(async move {
        let document = web_sys::window()
            .and_then(|w| w.document())
            .expect("no document");
        let canvas = document
            .get_element_by_id("earthcraft_canvas")
            .and_then(|c| c.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("no #earthcraft_canvas");
        let creator =
            move |cc: &::eframe::CreationContext<'_>| -> Result<Box<dyn ::eframe::App>, DynError> {
                CTX.with(|c| c.set(cc.egui_ctx.clone()).ok());
                Ok(Box::new(Web { app: *creator(cc)? }))
            };
        let runner = ::eframe::WebRunner::new();
        let started = runner
            .start(canvas, ::eframe::WebOptions::default(), Box::new(creator))
            .await;
        // The runner lives as long as the page.
        std::mem::forget(runner);
        if let Some(loading) = document.get_element_by_id("earthcraft_loading") {
            match started {
                Ok(()) => {
                    loading.remove();
                    call("ready", &[]);
                }
                Err(e) => {
                    let message = format!("EarthCraft could not start: {e:?}");
                    loading.set_text_content(Some(&message));
                    call("fail", &[message.into()]);
                }
            }
        }
    });
    Ok(())
}
