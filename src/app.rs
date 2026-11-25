use crate::backend::{Backend, Event, SyncState, TransferInfo};
use crate::config::{ClientCreds, Paths, Settings};
use crate::index::{Index, Query, Roots};
use crate::model::{DFile, Kind, SharedDrive};
use crate::pointer::PointerProbe;
use crate::theme;
use crate::widgets::{self, Button, Status};
use eframe::egui::{self, Align, Id, Key, Layout, Modifiers, RichText, Sense};
use egui_extras::{Column, TableBuilder};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

const SEARCH_LIMIT: usize = 5000;
const RECENT_LIMIT: usize = 500;

#[derive(Clone, PartialEq, Debug)]
enum View {
    MyDrive,
    Folder(String),
    Recent,
    Starred,
    Shared,
    Trash,
}

impl View {
    fn to_setting(&self) -> String {
        match self {
            View::MyDrive => "mydrive".into(),
            View::Recent => "recent".into(),
            View::Starred => "starred".into(),
            View::Shared => "shared".into(),
            View::Trash => "trash".into(),
            View::Folder(id) => format!("folder:{id}"),
        }
    }

    fn from_setting(s: &str) -> Option<View> {
        Some(match s {
            "mydrive" => View::MyDrive,
            "recent" => View::Recent,
            "starred" => View::Starred,
            "shared" => View::Shared,
            "trash" => View::Trash,
            _ => View::Folder(s.strip_prefix("folder:")?.to_string()),
        })
    }
}

#[derive(Clone, Copy, PartialEq)]
enum SortKey {
    Name,
    Modified,
    Size,
}

enum Screen {
    Setup,
    Main,
}

enum UiMsg {
    CredsFile(PathBuf),
    DownloadDir(PathBuf),
    Upload(Vec<PathBuf>, String),
}

enum Action {
    Click { id: String, ctrl: bool, shift: bool },
    Open(String),
    OpenInBrowser,
    CopyLink,
    CopyDownloadLink,
    CopyId,
    CopyName,
    Reveal(String),
    ShowLocal(String),
    ForgetLocal(String),
    Rename,
    Star(bool),
    Trash(bool),
    Download,
    Share,
    Cut,
    Paste,
    Nav(View),
    NewFolder,
    Upload,
    ContentSearch,
    Sort(SortKey),
    SetStartFolder(String),
    RetryUpload(usize),
    DismissFailed(usize),
}

#[derive(Clone)]
struct LocalCopy {
    path: PathBuf,
    /// Drive `modifiedTime` of the version that was downloaded.
    modified: String,
}

impl LocalCopy {
    fn outdated(&self, f: &DFile) -> bool {
        !self.modified.is_empty() && f.modified_time.as_deref().is_some_and(|m| m > self.modified.as_str())
    }
}

/// An upload shown as a row in its destination folder: running (with progress) or failed
/// (index into `App::failed`, error).
enum Pending<'a> {
    Uploading(&'a TransferInfo, Option<f32>),
    Failed(usize, &'a TransferInfo, &'a str),
}

/// A place files can be dropped onto: screen rect, folder id, display name.
type DropTarget = (egui::Rect, String, String);

struct Toast {
    text: String,
    error: bool,
    until: Instant,
}

pub struct App {
    paths: Paths,
    settings: Settings,
    backend: Backend,
    rx: Receiver<Event>,
    ui_tx: Sender<UiMsg>,
    ui_rx: Receiver<UiMsg>,
    screen: Screen,

    // data
    files: Arc<HashMap<String, Arc<DFile>>>,
    my_drive: Option<String>,
    drives: Vec<SharedDrive>,
    drive_names: HashMap<String, String>,
    index: Index,
    index_dirty: bool,
    last_build: Instant,
    build_cost: Duration,
    building: Option<Receiver<(Index, Duration)>>,
    reset_cursor: bool,
    cache_loaded: bool,

    // navigation / listing
    view: View,
    back: Vec<View>,
    forward: Vec<View>,
    query: String,
    last_query: String,
    rows: Vec<usize>,
    total_matches: usize,
    rows_dirty: bool,
    search_time: Duration,
    sort: Option<(SortKey, bool)>, // (key, descending)
    content: Option<(String, Vec<String>)>,
    content_pending: Option<String>,
    /// Limit search to the folder being viewed (and its subfolders).
    scope_folder: bool,

    // selection
    selected: HashSet<String>,
    cursor: Option<String>,
    anchor: Option<String>,
    scroll_to_cursor: bool,
    focus_search: bool,
    cut: Vec<(String, Vec<String>)>,
    local: HashMap<String, LocalCopy>,

    // drag & drop
    probe: Option<PointerProbe>,
    drop_targets: Vec<DropTarget>,
    drop_hover: Option<(String, String)>,

    // dialogs
    rename: Option<(String, String)>,
    new_folder: Option<String>,
    settings_open: bool,
    about_open: bool,
    dialog_focus: bool,
    /// Focus came from the keyboard (Tab), so the focus ring shows on every control.
    keyboard_focus: bool,

    // status
    sync: SyncState,
    synced_at: Option<Instant>,
    toasts: Vec<Toast>,
    transfers: Vec<(u64, TransferInfo, Option<f32>)>,
    /// Transfers that failed this session (shown in their row until retried or dismissed).
    failed: Vec<(TransferInfo, String)>,

    // setup screen
    creds: ClientCreds,
    setup_msg: Option<(String, bool)>,
    login_waiting: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Paths) -> anyhow::Result<App> {
        let ctx = cc.egui_ctx.clone();
        theme::install(&ctx);
        let settings = Settings::load(&paths);
        match settings.dark_mode {
            Some(true) => ctx.set_theme(egui::Theme::Dark),
            Some(false) => ctx.set_theme(egui::Theme::Light),
            None => {}
        }
        let (backend, rx) = Backend::new(ctx, paths.clone(), settings.poll_seconds)?;
        let (ui_tx, ui_rx) = channel();
        let screen = if backend.has_session() { Screen::Main } else { Screen::Setup };
        let creds = ClientCreds::load(&paths).unwrap_or_default();
        let demo = std::env::var_os("DRAY_DEMO").is_some();
        let screen = if demo { Screen::Main } else { screen };
        let start_view = settings.start_view.as_deref().and_then(View::from_setting).unwrap_or(View::MyDrive);
        let mut app = App {
            paths,
            settings,
            backend,
            rx,
            ui_tx,
            ui_rx,
            screen,
            files: Arc::new(HashMap::new()),
            my_drive: None,
            drives: Vec::new(),
            drive_names: HashMap::new(),
            index: Index::default(),
            index_dirty: false,
            last_build: Instant::now(),
            build_cost: Duration::ZERO,
            building: None,
            reset_cursor: true,
            cache_loaded: false,
            view: start_view,
            back: Vec::new(),
            forward: Vec::new(),
            query: String::new(),
            last_query: String::new(),
            rows: Vec::new(),
            total_matches: 0,
            rows_dirty: true,
            search_time: Duration::ZERO,
            sort: None,
            content: None,
            content_pending: None,
            scope_folder: false,
            selected: HashSet::new(),
            cursor: None,
            anchor: None,
            scroll_to_cursor: false,
            focus_search: true,
            cut: Vec::new(),
            local: HashMap::new(),
            probe: None,
            drop_targets: Vec::new(),
            drop_hover: None,
            rename: None,
            new_folder: None,
            settings_open: false,
            about_open: false,
            dialog_focus: false,
            keyboard_focus: false,
            sync: SyncState::Idle,
            synced_at: None,
            toasts: Vec::new(),
            transfers: Vec::new(),
            failed: Vec::new(),
            creds,
            setup_msg: None,
            login_waiting: false,
        };
        if demo {
            app.load_demo();
        }
        Ok(app)
    }

    /// `DRAY_DEMO=1`: fill the index with synthetic files (UI / performance testing, no network).
    fn load_demo(&mut self) {
        let words = ["Report", "Budget", "Photo", "Invoice", "Draft", "Final", "Meeting", "Notes", "Plan", "Design"];
        let exts = ["pdf", "docx", "xlsx", "png", "jpg", "mp4", "zip", "txt", "rs", "pptx"];
        let n: u32 = std::env::var("DRAY_DEMO").ok().and_then(|v| v.parse().ok()).filter(|&n| n > 1).unwrap_or(50_000);
        let files = Arc::make_mut(&mut self.files);
        for i in 0..n {
            let folder = i < n / 50;
            let parent = if i < 40 { "root".to_string() } else { format!("d{}", i % (n / 50).max(1)) };
            let w = words[(i % 10) as usize];
            let w2 = words[(i / 10 % 10) as usize];
            let name = if folder { format!("{w} {w2} {i}") } else { format!("{w}_{w2}-{i}.{}", exts[(i / 3 % 10) as usize]) };
            let day = 1 + i % 28;
            let month = 1 + i / 28 % 12;
            let year = 2026 - (i / 336 % 8);
            let f = DFile {
                id: format!("d{i}"),
                name,
                mime_type: if folder { crate::model::FOLDER_MIME.into() } else { "application/octet-stream".into() },
                parents: vec![parent],
                size: (!folder).then(|| ((i as u64 * 7919) % 50_000_000).to_string()),
                modified_time: Some(format!("{year}-{month:02}-{day:02}T10:00:00.000Z")),
                viewed_by_me_time: None,
                web_view_link: None,
                starred: i % 97 == 0,
                trashed: i % 211 == 0,
                owned_by_me: i % 13 != 0,
                shared: i % 17 == 0,
                drive_id: None,
                shortcut_details: None,
            };
            files.insert(f.id.clone(), Arc::new(f));
        }
        self.my_drive = Some("root".into());
        if let Ok(q) = std::env::var("DRAY_DEMO_QUERY") {
            self.query = q;
        }
        if let Ok(folder) = std::env::var("DRAY_DEMO_FOLDER") {
            self.view = View::Folder(folder);
            self.scope_folder = true;
        }
        // One running and one failed upload, so every row state can be looked at.
        let upload = |name: &str, size: u64| TransferInfo {
            upload: true,
            name: name.into(),
            target: "root".into(),
            size: Some(size),
            path: None,
        };
        self.transfers.push((u64::MAX, upload("site-photos.zip", 333_447_168), Some(0.62)));
        self.failed.push((upload("notes.txt", 2_048), "Couldn't reach Google".into()));
        self.synced_at = Some(Instant::now());
        self.cache_loaded = true;
        self.index_dirty = true;
    }

    // ------------------------------------------------------------------ events

    fn toast(&mut self, text: impl Into<String>, error: bool) {
        let secs = if error { 7 } else { 3 };
        self.toasts.push(Toast { text: text.into(), error, until: Instant::now() + Duration::from_secs(secs) });
        if self.toasts.len() > 6 {
            self.toasts.remove(0);
        }
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::CacheLoaded(list) => {
                    let files = Arc::make_mut(&mut self.files);
                    for f in list {
                        files.insert(f.id.clone(), Arc::new(f));
                    }
                    self.cache_loaded = true;
                    self.index_dirty = true;
                    // Startup folder deleted (or no longer visible)? Fall back to My Drive.
                    if let View::Folder(id) = &self.view {
                        if !self.files.is_empty() && !self.files.contains_key(id) && !self.drive_names.contains_key(id) {
                            self.view = View::MyDrive;
                            self.toast("Startup folder not found — showing My Drive", true);
                        }
                    }
                }
                Event::Upsert(list) => {
                    let files = Arc::make_mut(&mut self.files);
                    for f in list {
                        files.insert(f.id.clone(), Arc::new(f));
                    }
                    self.index_dirty = true;
                }
                Event::Remove(ids) => {
                    let files = Arc::make_mut(&mut self.files);
                    for id in ids {
                        files.remove(&id);
                    }
                    self.index_dirty = true;
                }
                Event::ResetFiles => {
                    self.files = Arc::new(HashMap::new());
                    self.my_drive = None;
                    self.index_dirty = true;
                }
                Event::Roots { my_drive, drives } => {
                    if my_drive.is_some() {
                        self.my_drive = my_drive;
                    }
                    self.drive_names = drives.iter().map(|d| (d.id.clone(), d.name.clone())).collect();
                    self.drives = drives;
                    self.index_dirty = true;
                }
                Event::Sync(s) => self.sync = s,
                Event::SyncedAt(t) => self.synced_at = Some(t),
                Event::LoggedIn => {
                    self.login_waiting = false;
                    self.screen = Screen::Main;
                    self.focus_search = true;
                    self.toast("Signed in — building local index…", false);
                }
                Event::AuthRequired(msg) => {
                    self.login_waiting = false;
                    self.screen = Screen::Setup;
                    if !msg.is_empty() {
                        self.setup_msg = Some((msg, true));
                    }
                }
                Event::Toast { text, error } => self.toast(text, error),
                Event::Progress { id, info, frac } => {
                    match self.transfers.iter_mut().find(|t| t.0 == id) {
                        Some(t) => t.2 = frac,
                        None => {
                            // A new attempt replaces an earlier failure of the same transfer.
                            self.failed.retain(|(f, _)| !(f.upload == info.upload && f.target == info.target && f.name == info.name));
                            self.transfers.push((id, info, frac));
                        }
                    }
                }
                Event::ProgressDone(id) => self.transfers.retain(|t| t.0 != id),
                Event::TransferFailed { info, error } => self.failed.push((info, error)),
                Event::Uploaded { file, single } => {
                    if single && self.settings.copy_link_after_upload {
                        ctx.copy_text(file.view_link());
                        self.toast(format!("Uploaded '{}' — link copied", file.name), false);
                    } else if single {
                        self.toast(format!("Uploaded '{}'", file.name), false);
                    }
                    self.select_only(file.id.clone());
                    self.scroll_to_cursor = true;
                }
                Event::ContentResults { query, ids } => {
                    if self.content_pending.as_deref() == Some(query.as_str()) {
                        self.content_pending = None;
                        let n = ids.len();
                        self.content = Some((query, ids));
                        self.rows_dirty = true;
                        if n == 0 {
                            self.toast("No content matches", false);
                        }
                    }
                }
                Event::CopyText { text, toast } => {
                    ctx.copy_text(text);
                    self.toast(toast, false);
                }
                Event::LocalCopies(list) => {
                    for (id, path, modified) in list {
                        self.local.insert(id, LocalCopy { path, modified });
                    }
                }
                Event::Downloaded { id, path, modified } => {
                    self.toast(format!("Saved {} — double-click to open it", path.display()), false);
                    self.local.insert(id, LocalCopy { path, modified });
                }
            }
        }
        while let Ok(msg) = self.ui_rx.try_recv() {
            match msg {
                UiMsg::CredsFile(path) => self.load_creds_file(path),
                UiMsg::Upload(paths, parent) => self.backend.upload(paths, parent),
                UiMsg::DownloadDir(dir) => {
                    self.settings.download_dir = Some(dir);
                    self.settings.save(&self.paths);
                }
            }
        }
    }

    /// Rebuild the search index on a background thread (the UI keeps using the old one
    /// meanwhile, so large syncs never stall a frame).
    fn maybe_rebuild(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.building {
            match rx.try_recv() {
                Ok((index, cost)) => {
                    self.index = index;
                    self.build_cost = cost;
                    self.last_build = Instant::now();
                    self.building = None;
                    self.rows_dirty = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(_) => self.building = None,
            }
        }
        if !self.index_dirty {
            return;
        }
        let min_gap = (self.build_cost * 2).clamp(Duration::from_millis(30), Duration::from_millis(500));
        if self.last_build.elapsed() < min_gap && !self.index.entries.is_empty() {
            ctx.request_repaint_after(min_gap - self.last_build.elapsed());
            return;
        }
        self.index_dirty = false;
        let files = self.files.clone();
        let my_drive = self.my_drive.clone();
        let drives = self.drive_names.clone();
        let (tx, rx) = channel();
        self.building = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let t = Instant::now();
            let index = Index::build(&files, &Roots { my_drive: my_drive.as_deref(), drives: &drives });
            let _ = tx.send((index, t.elapsed()));
            // Drop the snapshot here, off the UI thread.
            drop(files);
            ctx.request_repaint();
        });
    }

    // ------------------------------------------------------------------ listing

    fn current_folder(&self) -> Option<String> {
        match &self.view {
            View::MyDrive => self.my_drive.clone(),
            View::Folder(id) => Some(id.clone()),
            _ => None,
        }
    }

    fn is_searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    fn content_active(&self) -> bool {
        matches!(&self.content, Some((q, _)) if *q == self.query.trim())
    }

    fn recompute_rows(&mut self) {
        if self.query != self.last_query {
            self.last_query = self.query.clone();
            self.rows_dirty = true;
            self.reset_cursor = true;
        }
        if !self.rows_dirty {
            return;
        }
        self.rows_dirty = false;
        let t = Instant::now();
        let idx = &self.index;
        let mut rows: Vec<usize>;
        let mut relevance = false;
        if self.content_active() {
            let (_, ids) = self.content.as_ref().unwrap();
            rows = ids.iter().filter_map(|id| idx.by_id.get(id).copied()).collect();
            relevance = true;
        } else if self.is_searching() {
            let q = Query::parse(&self.query);
            let scope = if self.scope_folder { self.current_folder() } else { None };
            let (r, total) = idx.search(&q, SEARCH_LIMIT, scope.as_deref());
            rows = r;
            self.total_matches = total;
            relevance = true;
        } else {
            let live = |i: &usize| !idx.entries[*i].file.trashed;
            rows = match &self.view {
                View::MyDrive | View::Folder(_) => match self.current_folder() {
                    Some(f) => idx.children.get(&f).map(|v| v.iter().copied().filter(live).collect()).unwrap_or_default(),
                    None => Vec::new(),
                },
                View::Recent => {
                    let mut v: Vec<usize> = (0..idx.entries.len())
                        .filter(|i| live(i) && idx.entries[*i].kind != Kind::Folder)
                        .collect();
                    v.sort_unstable_by(|a, b| idx.entries[*b].file.recency().cmp(idx.entries[*a].file.recency()));
                    v.truncate(RECENT_LIMIT);
                    relevance = true;
                    v
                }
                View::Starred => (0..idx.entries.len()).filter(|i| live(i) && idx.entries[*i].file.starred).collect(),
                View::Shared => {
                    let mut v: Vec<usize> = (0..idx.entries.len())
                        .filter(|i| {
                            let f = &idx.entries[*i].file;
                            live(i) && !f.owned_by_me && f.drive_id.is_none()
                        })
                        .collect();
                    v.sort_unstable_by(|a, b| idx.entries[*b].file.recency().cmp(idx.entries[*a].file.recency()));
                    relevance = true;
                    v
                }
                View::Trash => (0..idx.entries.len())
                    .filter(|i| {
                        let f = &idx.entries[*i].file;
                        f.trashed
                            && f.parents.first().and_then(|p| self.files.get(p)).is_none_or(|p| !p.trashed)
                    })
                    .collect(),
            };
            self.total_matches = rows.len();
        }
        let sort = match self.sort {
            Some(s) => Some(s),
            None if relevance => None,
            None => Some((SortKey::Name, false)),
        };
        if let Some((key, desc)) = sort {
            rows.sort_by(|&a, &b| {
                let (ea, eb) = (&idx.entries[a], &idx.entries[b]);
                let fa = ea.kind == Kind::Folder;
                let fb = eb.kind == Kind::Folder;
                let ord = match key {
                    SortKey::Name => ea.file.name.to_lowercase().cmp(&eb.file.name.to_lowercase()),
                    SortKey::Modified => ea.file.modified_time.cmp(&eb.file.modified_time),
                    SortKey::Size => ea.file.size_bytes().cmp(&eb.file.size_bytes()),
                };
                fb.cmp(&fa).then(if desc { ord.reverse() } else { ord })
            });
        }
        self.rows = rows;
        self.search_time = t.elapsed();
        // After a new query / navigation, put the cursor on the first row.
        if (self.reset_cursor || self.cursor.is_none()) && !self.rows.is_empty() {
            self.reset_cursor = false;
            self.cursor = self.rows.first().map(|&i| self.index.entries[i].file.id.clone());
            self.selected.clear();
            if let Some(c) = &self.cursor {
                if self.is_searching() {
                    self.selected.insert(c.clone());
                }
            }
            self.anchor = self.cursor.clone();
            self.scroll_to_cursor = true;
        }
    }

    fn cursor_pos(&self) -> Option<usize> {
        let c = self.cursor.as_ref()?;
        self.rows.iter().position(|&i| self.index.entries[i].file.id == *c)
    }

    fn row_id(&self, pos: usize) -> Option<String> {
        self.rows.get(pos).map(|&i| self.index.entries[i].file.id.clone())
    }

    fn select_only(&mut self, id: String) {
        self.reset_cursor = false;
        self.selected.clear();
        self.selected.insert(id.clone());
        self.anchor = Some(id.clone());
        self.cursor = Some(id);
    }

    fn move_cursor(&mut self, delta: isize, extend: bool) {
        if self.rows.is_empty() {
            return;
        }
        let pos = self.cursor_pos().map(|p| p as isize).unwrap_or(-1);
        let new = (pos + delta).clamp(0, self.rows.len() as isize - 1) as usize;
        let id = self.row_id(new).unwrap();
        if extend {
            self.cursor = Some(id);
            self.select_range();
        } else {
            self.select_only(id);
        }
        self.scroll_to_cursor = true;
    }

    fn select_range(&mut self) {
        let pos_of = |id: &Option<String>| {
            id.as_ref().and_then(|c| self.rows.iter().position(|&i| self.index.entries[i].file.id == *c))
        };
        let (Some(a), Some(b)) = (pos_of(&self.anchor), pos_of(&self.cursor)) else { return };
        let (lo, hi) = (a.min(b), a.max(b));
        self.selected = (lo..=hi).filter_map(|p| self.row_id(p)).collect();
    }

    /// Selected files, in display order; falls back to the cursor row.
    fn targets(&self) -> Vec<Arc<DFile>> {
        let mut out: Vec<Arc<DFile>> = self
            .rows
            .iter()
            .map(|&i| &self.index.entries[i].file)
            .filter(|f| self.selected.contains(&f.id))
            .cloned()
            .collect();
        if out.is_empty() {
            if let Some(f) = self.cursor.as_ref().and_then(|c| self.files.get(c)) {
                out.push(f.clone());
            }
        }
        out
    }

    fn navigate(&mut self, view: View) {
        if view == self.view && !self.is_searching() {
            return;
        }
        self.back.push(std::mem::replace(&mut self.view, view));
        self.forward.clear();
        self.after_nav();
    }

    fn after_nav(&mut self) {
        self.query.clear();
        self.last_query.clear();
        self.content = None;
        self.sort = None;
        self.rows_dirty = true;
        self.reset_cursor = true;
        self.cursor = None;
        self.selected.clear();
    }

    fn go_back(&mut self) {
        if self.is_searching() {
            self.query.clear();
            self.rows_dirty = true;
            return;
        }
        if let Some(v) = self.back.pop() {
            self.forward.push(std::mem::replace(&mut self.view, v));
            self.after_nav();
        }
    }

    fn go_forward(&mut self) {
        if let Some(v) = self.forward.pop() {
            self.back.push(std::mem::replace(&mut self.view, v));
            self.after_nav();
        }
    }

    fn go_up(&mut self) {
        let Some(cur) = self.current_folder() else { return };
        let parent = self.files.get(&cur).and_then(|f| f.parents.first().cloned());
        let child = cur.clone();
        match parent {
            Some(p) if Some(&p) == self.my_drive.as_ref() => self.navigate(View::MyDrive),
            Some(p) => self.navigate(View::Folder(p)),
            None => return,
        }
        self.select_only(child);
        self.scroll_to_cursor = true;
    }

    fn folder_view(&self, id: &str) -> View {
        if Some(id) == self.my_drive.as_deref() { View::MyDrive } else { View::Folder(id.to_string()) }
    }

    fn open(&mut self, id: &str) {
        let Some(f) = self.files.get(id).cloned() else { return };
        if let Some(target) = f.folder_target() {
            let v = self.folder_view(target);
            self.navigate(v);
        } else {
            // Downloaded before? Open the local copy instead of the web page.
            if let Some(lc) = self.local.get(&f.id).cloned() {
                if lc.path.exists() {
                    if let Err(e) = open::that_detached(&lc.path) {
                        self.toast(format!("Couldn't open {}: {e}", lc.path.display()), true);
                    } else if lc.outdated(&f) {
                        self.toast(format!("Drive has a newer version of '{}' — Ctrl+S updates your copy", f.name), false);
                    }
                    return;
                }
                // File was moved or deleted locally: forget it and fall back to the web.
                self.local.remove(&f.id);
                self.backend.forget_local_copy(&f.id);
            }
            let link = match &f.shortcut_details {
                Some(s) if !s.target_id.is_empty() => self
                    .files
                    .get(&s.target_id)
                    .map(|t| t.view_link())
                    .unwrap_or_else(|| format!("https://drive.google.com/open?id={}", s.target_id)),
                _ => f.view_link(),
            };
            let _ = open::that_detached(link);
        }
    }

    fn reveal(&mut self, id: &str) {
        let Some(f) = self.files.get(id).cloned() else { return };
        let Some(parent) = f.parents.first() else {
            self.toast("This item has no visible parent folder", true);
            return;
        };
        let v = self.folder_view(parent);
        self.navigate(v);
        self.select_only(f.id.clone());
        self.scroll_to_cursor = true;
    }

    fn copy_links(&mut self, ctx: &egui::Context, download: bool) {
        let t = self.targets();
        if t.is_empty() {
            return;
        }
        let text = t
            .iter()
            .map(|f| if download { f.download_link() } else { link_for(f) })
            .collect::<Vec<_>>()
            .join("\n");
        ctx.copy_text(text);
        let what = if download { "Download link" } else { "Link" };
        if t.len() == 1 {
            self.toast(format!("{what} copied: {}", t[0].name), false);
        } else {
            self.toast(format!("{} {what}s copied", t.len()), false);
        }
    }

    fn folder_name(&self, id: &str) -> String {
        if Some(id) == self.my_drive.as_deref() {
            return "My Drive".into();
        }
        self.drive_names
            .get(id)
            .cloned()
            .or_else(|| self.files.get(id).map(|f| f.name.clone()))
            .unwrap_or_else(|| "this folder".into())
    }

    fn view_label(&self, v: &View) -> String {
        match v {
            View::MyDrive => "My Drive".into(),
            View::Recent => "Recent".into(),
            View::Starred => "Starred".into(),
            View::Shared => "Shared with me".into(),
            View::Trash => "Trash".into(),
            View::Folder(id) => self.folder_name(id),
        }
    }

    /// The folder search is limited to, if the scope toggle is on and a folder is open.
    fn search_scope(&self) -> Option<String> {
        if self.scope_folder { self.current_folder() } else { None }
    }

    fn toggle_scope(&mut self) {
        if self.current_folder().is_some() {
            self.scope_folder = !self.scope_folder;
            self.rows_dirty = true;
            self.reset_cursor = true;
            self.focus_search = true;
        }
    }

    fn download_dir(&self) -> PathBuf {
        self.settings.download_dir.clone().unwrap_or_else(Backend::default_download_dir)
    }

    fn upload_target(&self) -> Option<String> {
        self.current_folder().or_else(|| self.my_drive.clone())
    }

    fn apply(&mut self, ctx: &egui::Context, a: Action) {
        match a {
            Action::Click { id, ctrl, shift } => {
                if shift {
                    self.cursor = Some(id);
                    self.select_range();
                } else if ctrl {
                    if !self.selected.remove(&id) {
                        self.selected.insert(id.clone());
                    }
                    self.anchor = Some(id.clone());
                    self.cursor = Some(id);
                } else {
                    self.select_only(id);
                }
            }
            Action::Open(id) => self.open(&id),
            Action::OpenInBrowser => {
                for f in self.targets().iter().take(10) {
                    let _ = open::that_detached(link_for(f));
                }
            }
            Action::CopyLink => self.copy_links(ctx, false),
            Action::CopyDownloadLink => self.copy_links(ctx, true),
            Action::CopyId => {
                let ids: Vec<String> = self.targets().iter().map(|f| f.id.clone()).collect();
                ctx.copy_text(ids.join("\n"));
                self.toast("ID copied", false);
            }
            Action::CopyName => {
                let names: Vec<String> = self.targets().iter().map(|f| f.name.clone()).collect();
                ctx.copy_text(names.join("\n"));
                self.toast("Name copied", false);
            }
            Action::Reveal(id) => self.reveal(&id),
            Action::Rename => {
                if let Some(f) = self.targets().first() {
                    self.rename = Some((f.id.clone(), f.name.clone()));
                    self.dialog_focus = true;
                }
            }
            Action::Star(on) => {
                let ids = self.targets().iter().map(|f| f.id.clone()).collect();
                self.backend.set_starred(ids, on);
            }
            Action::Trash(on) => {
                let ids: Vec<String> = self.targets().iter().map(|f| f.id.clone()).collect();
                if !ids.is_empty() {
                    self.backend.set_trashed(ids, on);
                }
            }
            Action::Download => {
                let files: Vec<(DFile, Option<PathBuf>)> = self
                    .targets()
                    .iter()
                    .map(|f| {
                        let existing = self.local.get(&f.id).map(|l| l.path.clone()).filter(|p| p.exists());
                        ((**f).clone(), existing)
                    })
                    .collect();
                if !files.is_empty() {
                    self.backend.download(files, self.download_dir());
                }
            }
            Action::ShowLocal(id) => {
                if let Some(lc) = self.local.get(&id) {
                    reveal_in_file_manager(&lc.path);
                }
            }
            Action::ForgetLocal(id) => {
                self.local.remove(&id);
                self.backend.forget_local_copy(&id);
                self.toast("Local copy forgotten (the file itself was not deleted)", false);
            }
            Action::Share => {
                if let Some(f) = self.targets().first() {
                    self.backend.share_and_copy((**f).clone());
                }
            }
            Action::Cut => {
                self.cut = self.targets().iter().map(|f| (f.id.clone(), f.parents.clone())).collect();
                if !self.cut.is_empty() {
                    self.toast(format!("{} item(s) ready to move — open a folder and press Ctrl+V", self.cut.len()), false);
                }
            }
            Action::Paste => {
                if self.cut.is_empty() {
                    return;
                }
                match self.current_folder() {
                    Some(dest) => {
                        let items = std::mem::take(&mut self.cut);
                        self.backend.move_to(items, dest);
                    }
                    None => self.toast("Open a folder to paste into", true),
                }
            }
            Action::Nav(v) => self.navigate(v),
            Action::NewFolder => {
                if self.upload_target().is_some() {
                    self.new_folder = Some(String::from("New folder"));
                    self.dialog_focus = true;
                }
            }
            Action::Upload => {
                let Some(parent) = self.upload_target() else {
                    self.toast("Still syncing — try again in a moment", true);
                    return;
                };
                let tx = self.ui_tx.clone();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    if let Some(paths) = rfd::FileDialog::new().set_title("Upload files").pick_files() {
                        let _ = tx.send(UiMsg::Upload(paths, parent));
                        ctx.request_repaint();
                    }
                });
            }
            Action::ContentSearch => {
                let q = self.query.trim().to_string();
                if !q.is_empty() {
                    self.content_pending = Some(q.clone());
                    self.backend.content_search(q);
                }
            }
            Action::RetryUpload(i) => {
                if i < self.failed.len() {
                    let (info, _) = self.failed.remove(i);
                    if let Some(path) = info.path {
                        self.backend.upload(vec![path], info.target);
                    }
                }
            }
            Action::DismissFailed(i) => {
                if i < self.failed.len() {
                    self.failed.remove(i);
                }
            }
            Action::SetStartFolder(id) => {
                let v = self.folder_view(&id);
                self.settings.start_view = Some(v.to_setting());
                self.settings.save(&self.paths);
                self.toast(format!("“{}” will open at startup", self.folder_name(&id)), false);
            }
            Action::Sort(key) => {
                self.sort = match self.sort {
                    Some((k, desc)) if k == key => Some((k, !desc)),
                    _ => Some((key, key != SortKey::Name)),
                };
                self.rows_dirty = true;
            }
        }
    }

    // ------------------------------------------------------------------ keyboard

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.rename.is_some() || self.new_folder.is_some() {
            return;
        }
        let search_focused = ctx.memory(|m| m.has_focus(search_id()));
        let text_focused = ctx.egui_wants_keyboard_input();
        let other_text_focused = text_focused && !search_focused;
        if other_text_focused {
            return;
        }
        let mut actions = Vec::new();
        let ctrl = Modifiers::COMMAND;
        let cs = Modifiers::COMMAND | Modifiers::SHIFT;
        let mut scope_toggle = false;
        // Ctrl+C copies the selected file link(s) unless text is actually selected in the box.
        // NB: must be read *before* `input_mut`: egui's context lock is not re-entrant, and
        // touching ctx inside the closure deadlocks the UI thread.
        let has_text_selection = search_focused
            && egui::text_edit::TextEditState::load(ctx, search_id())
                .and_then(|s| s.cursor.char_range())
                .is_some_and(|r| !r.is_empty());
        ctx.input_mut(|i| {
            if i.consume_key(cs, Key::F) {
                scope_toggle = true;
            }
            if i.consume_key(ctrl, Key::F) || i.consume_key(ctrl, Key::K) || i.consume_key(ctrl, Key::L) {
                self.focus_search = true;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                self.move_cursor(1, false);
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                self.move_cursor(-1, false);
            }
            if i.consume_key(Modifiers::SHIFT, Key::ArrowDown) {
                self.move_cursor(1, true);
            }
            if i.consume_key(Modifiers::SHIFT, Key::ArrowUp) {
                self.move_cursor(-1, true);
            }
            if i.consume_key(Modifiers::NONE, Key::PageDown) {
                self.move_cursor(15, false);
            }
            if i.consume_key(Modifiers::NONE, Key::PageUp) {
                self.move_cursor(-15, false);
            }
            if !search_focused {
                if i.consume_key(Modifiers::NONE, Key::Home) {
                    self.move_cursor(-(self.rows.len() as isize), false);
                }
                if i.consume_key(Modifiers::NONE, Key::End) {
                    self.move_cursor(self.rows.len() as isize, false);
                }
            }
            if i.consume_key(Modifiers::SHIFT, Key::Enter) {
                actions.push(Action::ContentSearch);
            }
            if i.consume_key(ctrl, Key::Enter) {
                if let Some(c) = self.cursor.clone() {
                    actions.push(Action::Reveal(c));
                }
            }
            if i.consume_key(Modifiers::NONE, Key::Enter) {
                if let Some(c) = self.cursor.clone() {
                    actions.push(Action::Open(c));
                }
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                if self.is_searching() {
                    self.query.clear();
                    self.content = None;
                    self.rows_dirty = true;
                } else {
                    self.selected.clear();
                }
                self.focus_search = true;
            }
            if i.consume_key(cs, Key::C) {
                actions.push(Action::CopyLink);
            }
            if i.consume_key(cs, Key::D) {
                actions.push(Action::CopyDownloadLink);
            }
            if i.consume_key(cs, Key::S) {
                actions.push(Action::Share);
            }
            if i.consume_key(cs, Key::N) {
                actions.push(Action::NewFolder);
            }
            if i.consume_key(ctrl, Key::U) {
                actions.push(Action::Upload);
            }
            if i.consume_key(ctrl, Key::S) {
                actions.push(Action::Download);
            }
            if i.consume_key(ctrl, Key::O) {
                actions.push(Action::OpenInBrowser);
            }
            if i.consume_key(Modifiers::NONE, Key::F5) || i.consume_key(ctrl, Key::R) {
                self.backend.sync_now();
                self.toast("Syncing…", false);
            }
            if i.consume_key(Modifiers::NONE, Key::F2) {
                actions.push(Action::Rename);
            }
            if i.consume_key(Modifiers::ALT, Key::ArrowLeft) || i.consume_key(Modifiers::NONE, Key::BrowserBack) {
                self.go_back();
            }
            if i.consume_key(Modifiers::ALT, Key::ArrowRight) {
                self.go_forward();
            }
            if i.consume_key(Modifiers::ALT, Key::ArrowUp) {
                self.go_up();
            }
            // Keys that conflict with text editing only apply when the box is empty / unfocused.
            let free = !search_focused || self.query.is_empty();
            if free {
                if i.consume_key(Modifiers::NONE, Key::Backspace) {
                    if self.view == View::MyDrive || !matches!(self.view, View::Folder(_)) {
                        self.go_back();
                    } else {
                        self.go_up();
                    }
                }
                // Require an explicit selection so a stray Delete can't trash the default row.
                if i.consume_key(Modifiers::NONE, Key::Delete) && !self.selected.is_empty() {
                    actions.push(Action::Trash(self.view != View::Trash));
                }
            }
            if !search_focused {
                if i.consume_key(ctrl, Key::A) {
                    self.selected = self.rows.iter().map(|&r| self.index.entries[r].file.id.clone()).collect();
                }
                // Type-to-search: printable text goes to the search box.
                let mut typed = String::new();
                i.events.retain(|e| match e {
                    egui::Event::Text(t) => {
                        typed.push_str(t);
                        false
                    }
                    _ => true,
                });
                if !typed.is_empty() && !(typed == "/" && self.query.is_empty()) {
                    self.query.push_str(&typed);
                    self.focus_search = true;
                }
            }
            // Copy / cut / paste arrive as events, not keys.
            let mut copy = false;
            let mut cut = false;
            let mut paste = false;
            i.events.retain(|e| match e {
                egui::Event::Copy if !has_text_selection => {
                    copy = true;
                    false
                }
                egui::Event::Cut if !search_focused || self.query.is_empty() => {
                    cut = true;
                    false
                }
                egui::Event::Paste(_) if (!search_focused || self.query.is_empty()) && !self.cut.is_empty() => {
                    paste = true;
                    false
                }
                _ => true,
            });
            if copy {
                actions.push(Action::CopyLink);
            }
            if cut {
                actions.push(Action::Cut);
            }
            if paste {
                actions.push(Action::Paste);
            }
        });
        if scope_toggle {
            self.toggle_scope();
        }
        for a in actions {
            self.apply(ctx, a);
        }
    }

    // ------------------------------------------------------------------ UI pieces

    fn load_creds_file(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path).map_err(anyhow::Error::from).and_then(|t| ClientCreds::parse(&t)) {
            Ok(c) => {
                self.creds = c;
                self.setup_msg = Some(("Credentials loaded. Click “Sign in with Google”.".into(), false));
            }
            Err(e) => self.setup_msg = Some((format!("{}: {e:#}", path.display()), true)),
        }
    }

    fn setup_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if let Some(p) = dropped.into_iter().next() {
            self.load_creds_file(p);
        }
        let p = theme::of(ui);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.surface)).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(theme::SPACE_12);
                    ui.label(RichText::new("Dray").font(theme::semibold_font(theme::FONT_TITLE)).color(p.ink));
                    ui.label(RichText::new("A fast local front-end for Google Drive").weak());
                    ui.add_space(theme::SPACE_6);
                });
                ui.vertical_centered(|ui| {
                    let width = 620.0;
                    ui.set_max_width(width);
                    egui::Frame::new()
                        .fill(p.surface_raised)
                        .stroke(egui::Stroke::new(1.0, p.border))
                        .corner_radius(theme::radius(theme::RADIUS_LG))
                        .inner_margin(theme::SPACE_8)
                        .show(ui, |ui| {
                            ui.set_width(width - 2.0 * theme::SPACE_8);
                            ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                                ui.heading("One-time setup: your own Google OAuth client");
                                ui.add_space(theme::SPACE_2);
                                for line in [
                                    "1. In Google Cloud Console create a project and enable the Google Drive API.",
                                    "2. Configure the OAuth consent screen (External; add yourself as a test user).",
                                    "3. Credentials → Create credentials → OAuth client ID → Desktop app.",
                                    "4. Download the JSON and drop it on this window (or browse / paste below).",
                                ] {
                                    ui.label(line);
                                }
                                ui.add_space(theme::SPACE_4);
                                ui.horizontal(|ui| {
                                    if ui.add(Button::secondary("Open Cloud Console")).clicked() {
                                        let _ = open::that_detached("https://console.cloud.google.com/apis/credentials");
                                    }
                                    if ui.add(Button::secondary("Enable Drive API")).clicked() {
                                        let _ = open::that_detached(
                                            "https://console.cloud.google.com/apis/library/drive.googleapis.com",
                                        );
                                    }
                                    if ui.add(Button::secondary("Browse for JSON…")).clicked() {
                                        let tx = self.ui_tx.clone();
                                        let ctx = ctx.clone();
                                        std::thread::spawn(move || {
                                            if let Some(p) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file() {
                                                let _ = tx.send(UiMsg::CredsFile(p));
                                                ctx.request_repaint();
                                            }
                                        });
                                    }
                                });
                                ui.add_space(theme::SPACE_4);
                                egui::Grid::new("creds").num_columns(2).spacing([theme::SPACE_3, theme::SPACE_2]).show(ui, |ui| {
                                    ui.label("Client ID");
                                    ui.add(text_input(&mut self.creds.client_id).desired_width(420.0));
                                    ui.end_row();
                                    ui.label("Client secret");
                                    ui.add(text_input(&mut self.creds.client_secret).password(true).desired_width(420.0));
                                    ui.end_row();
                                });
                                ui.add_space(theme::SPACE_4);
                                ui.horizontal(|ui| {
                                    let ready = self.creds.is_complete() && !self.login_waiting;
                                    if ui.add_enabled(ready, Button::primary("Sign in with Google")).clicked() {
                                        self.login_waiting = true;
                                        self.setup_msg = None;
                                        self.backend.login(self.creds.clone());
                                    }
                                    if self.login_waiting {
                                        ui.add(egui::Spinner::new().color(p.ink_muted));
                                        ui.label("Waiting for you to finish in the browser…");
                                    }
                                });
                                if let Some((msg, err)) = &self.setup_msg {
                                    ui.add_space(theme::SPACE_2);
                                    let color = if *err { p.danger } else { p.ink };
                                    ui.colored_label(color, msg);
                                }
                                ui.add_space(theme::SPACE_4);
                                ui.label(RichText::new("Tokens are stored in (readable only by you):").small().weak());
                                ui.label(widgets::mono_small(self.paths.config_dir.display().to_string()).weak());
                                ui.label(RichText::new("Metadata cache:").small().weak());
                                ui.label(widgets::mono_small(self.paths.cache_file().display().to_string()).weak());
                            });
                        });
                });
                ui.add_space(theme::SPACE_8);
            });
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let mut targets: Vec<DropTarget> = Vec::new();
        let p = theme::of(ui);
        let frame = egui::Frame::new().fill(p.surface_sunken).inner_margin(egui::Margin::same(theme::SPACE_2 as i8));
        let mut toggle_settings = false;
        let mut open_about = false;
        egui::Panel::left("sidebar").resizable(true).default_size(208.0).frame(frame).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = theme::SPACE_1 / 2.0;
            let cur = self.view.clone();
            let searching = self.is_searching();
            let hover = self.drop_hover.as_ref().map(|h| h.0.clone());
            let mut item = |ui: &mut egui::Ui, label: &str, v: View, actions: &mut Vec<Action>| {
                let drop_id = match &v {
                    View::MyDrive => self.my_drive.clone(),
                    View::Folder(id) => Some(id.clone()),
                    _ => None,
                };
                let selected = !searching && cur == v;
                let drop = drop_id.is_some() && drop_id == hover;
                let r = widgets::nav_item(ui, label, selected, drop);
                if r.clicked() {
                    actions.push(Action::Nav(v));
                }
                if let Some(id) = drop_id {
                    targets.push((r.rect, id, label.to_string()));
                }
            };
            item(ui, "🏠  My Drive", View::MyDrive, actions);
            item(ui, "🕘  Recent", View::Recent, actions);
            item(ui, "⭐  Starred", View::Starred, actions);
            item(ui, "👥  Shared with me", View::Shared, actions);
            item(ui, "🗑  Trash", View::Trash, actions);
            if !self.drives.is_empty() {
                ui.add_space(theme::SPACE_4);
                ui.horizontal(|ui| {
                    ui.add_space(theme::SPACE_2);
                    widgets::section_label(ui, "Shared drives");
                });
                ui.add_space(theme::SPACE_1);
                for d in self.drives.clone() {
                    item(ui, &format!("🖴  {}", d.name), View::Folder(d.id.clone()), actions);
                }
            }
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = theme::SPACE_1;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme::SPACE_1;
                    toggle_settings = ui.add(Button::ghost("⚙ Settings")).clicked();
                    open_about = ui.add(Button::ghost("About")).clicked();
                });
                ui.horizontal(|ui| {
                    ui.add_space(theme::SPACE_2);
                    ui.label(RichText::new(format!("{} items cached", self.files.len())).small().weak());
                });
            });
        });
        if toggle_settings {
            self.settings_open = !self.settings_open;
        }
        if open_about {
            self.about_open = true;
        }
        self.drop_targets.extend(targets);
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let p = theme::of(ui);
        let frame = egui::Frame::new()
            .fill(p.surface)
            .inner_margin(egui::Margin::symmetric(theme::SPACE_3 as i8, theme::SPACE_2 as i8));
        egui::Panel::top("top").frame(frame).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::SPACE_1;
                if ui.add_enabled(!self.back.is_empty() || self.is_searching(), Button::ghost("◀")).on_hover_text("Back (Alt+←)").clicked() {
                    self.go_back();
                }
                if ui.add_enabled(!self.forward.is_empty(), Button::ghost("▶")).on_hover_text("Forward (Alt+→)").clicked() {
                    self.go_forward();
                }
                if ui.add_enabled(matches!(self.view, View::Folder(_)), Button::ghost("⬆")).on_hover_text("Parent folder (Alt+↑)").clicked() {
                    self.go_up();
                }
                ui.add_space(theme::SPACE_2);
                ui.spacing_mut().item_spacing.x = theme::SPACE_2;
                let folder = self.current_folder();
                let scope = self.search_scope();
                let right_w = if folder.is_some() { 440.0 } else { 320.0 };
                let w = (ui.available_width() - right_w).max(200.0);
                let hint = match &scope {
                    Some(id) => format!("🔍  Search in “{}” and its subfolders", self.folder_name(id)),
                    None => "🔍  Search all files   (type:pdf  ext:psd  is:starred  \"exact phrase\")".into(),
                };
                let resp = ui.add(text_input(&mut self.query).id(search_id()).hint_text(hint).desired_width(w));
                if let Some(id) = &folder {
                    let on = scope.is_some();
                    let tip = if on {
                        format!("Searching only “{}” and its subfolders — click to search everything (Ctrl+Shift+F)", self.folder_name(id))
                    } else {
                        format!("Only search “{}” and its subfolders (Ctrl+Shift+F)", self.folder_name(id))
                    };
                    if ui.add(Button::secondary("📁 This folder").selected(on)).on_hover_text(tip).clicked() {
                        self.toggle_scope();
                    }
                }
                if self.focus_search {
                    resp.request_focus();
                    self.focus_search = false;
                }
                if self.is_searching() {
                    let busy = self.content_pending.is_some();
                    let label = if busy { "Searching…" } else { "In contents" };
                    if ui
                        .add_enabled(!busy, Button::secondary(label))
                        .on_hover_text("Search inside file contents on Drive (Shift+Enter)")
                        .clicked()
                    {
                        actions.push(Action::ContentSearch);
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(Button::primary("⬆ Upload")).on_hover_text("Upload files (Ctrl+U) — or drop files on the window").clicked() {
                        actions.push(Action::Upload);
                    }
                    if ui.add(Button::secondary("➕ Folder")).on_hover_text("New folder (Ctrl+Shift+N)").clicked() {
                        actions.push(Action::NewFolder);
                    }
                    let syncing = matches!(self.sync, SyncState::Initial { .. } | SyncState::Incremental);
                    if syncing {
                        ui.add_sized([theme::CONTROL_H, theme::CONTROL_H], egui::Spinner::new().size(theme::ICON).color(p.ink_muted));
                    } else if ui.add(Button::ghost("⟳")).on_hover_text("Sync now (F5)").clicked() {
                        self.backend.sync_now();
                    }
                });
            });
            ui.add_space(theme::SPACE_2);
            // Breadcrumb / heading
            ui.horizontal(|ui| {
                ui.set_min_height(theme::CONTROL_H);
                if self.content_active() {
                    ui.label(RichText::new(format!("Content matches for “{}”", self.query.trim())).font(theme::semibold_font(theme::FONT_UI_ROW)));
                    ui.label(RichText::new("(from Drive)").weak());
                } else if self.is_searching() {
                    let n = self.total_matches;
                    ui.label(RichText::new(format!("{n} result{}", if n == 1 { "" } else { "s" })).font(theme::semibold_font(theme::FONT_UI_ROW)));
                    if let Some(id) = self.search_scope() {
                        ui.label(format!("in “{}”", self.folder_name(&id)));
                        if ui.add(Button::ghost("✖ search everywhere")).clicked() {
                            self.toggle_scope();
                        }
                    }
                    if self.total_matches > self.rows.len() {
                        ui.label(RichText::new(format!("(showing top {})", self.rows.len())).weak());
                    }
                    ui.label(widgets::mono_small(format!("{:.1} ms", self.search_time.as_secs_f64() * 1000.0)).weak());
                } else {
                    let mut targets = Vec::new();
                    self.breadcrumb(ui, actions, &mut targets);
                    self.drop_targets.extend(targets);
                }
            });
        });
    }

    fn breadcrumb(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>, targets: &mut Vec<DropTarget>) {
        let p = theme::of(ui);
        let current = |ui: &mut egui::Ui, name: &str| ui.label(RichText::new(name).font(theme::semibold_font(theme::FONT_UI_ROW)).color(p.ink));
        let title = match &self.view {
            View::Recent => Some("Recent"),
            View::Starred => Some("Starred"),
            View::Shared => Some("Shared with me"),
            View::Trash => Some("Trash"),
            _ => None,
        };
        if let Some(t) = title {
            current(ui, t);
            return;
        }
        let Some(mut id) = self.current_folder() else {
            current(ui, "My Drive");
            if !self.cache_loaded || matches!(self.sync, SyncState::Initial { .. }) {
                ui.add(egui::Spinner::new().size(theme::ICON).color(p.ink_muted));
            }
            return;
        };
        let mut crumbs: Vec<(String, View)> = Vec::new();
        for _ in 0..64 {
            if Some(&id) == self.my_drive.as_ref() {
                crumbs.push(("My Drive".into(), View::MyDrive));
                break;
            }
            if let Some(name) = self.drive_names.get(&id) {
                crumbs.push((name.clone(), View::Folder(id.clone())));
                break;
            }
            match self.files.get(&id) {
                Some(f) => {
                    crumbs.push((f.name.clone(), View::Folder(id.clone())));
                    match f.parents.first() {
                        Some(p) => id = p.clone(),
                        None => break,
                    }
                }
                None => {
                    crumbs.push(("…".into(), View::Folder(id.clone())));
                    break;
                }
            }
        }
        crumbs.reverse();
        let n = crumbs.len();
        for (i, (name, v)) in crumbs.into_iter().enumerate() {
            if i > 0 {
                ui.label(RichText::new("›").color(p.ink_muted));
            }
            let folder_id = match &v {
                View::MyDrive => self.my_drive.clone(),
                View::Folder(id) => Some(id.clone()),
                _ => None,
            };
            let hovered = folder_id.is_some() && folder_id == self.drop_hover.as_ref().map(|h| h.0.clone());
            let r = if i + 1 == n {
                current(ui, &name)
            } else {
                let mut text = RichText::new(name.clone());
                if hovered {
                    text = text.underline().strong();
                }
                let r = ui.link(text);
                if r.clicked() {
                    actions.push(Action::Nav(v));
                }
                r
            };
            if let Some(id) = folder_id {
                targets.push((r.rect.expand(4.0), id, name));
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let p = theme::of(ui);
        let frame = egui::Frame::new()
            .fill(p.surface)
            .inner_margin(egui::Margin::symmetric(theme::SPACE_3 as i8, theme::SPACE_1 as i8));
        egui::Panel::bottom("status").frame(frame).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(theme::ROW_COMPACT);
                let note = |ui: &mut egui::Ui, text: String| ui.label(RichText::new(text).small().weak());
                match &self.sync {
                    SyncState::Initial { files } => {
                        widgets::status_pill(ui, Status::Syncing);
                        note(ui, format!("Indexing Drive… {files} items"));
                    }
                    SyncState::Incremental => {
                        widgets::status_pill(ui, Status::Syncing);
                        note(ui, "Applying changes…".into());
                    }
                    SyncState::Offline(e) => {
                        widgets::status_pill(ui, Status::Offline).on_hover_text(e);
                        note(ui, "Offline / sync error".into()).on_hover_text(e);
                    }
                    SyncState::Idle => match self.synced_at {
                        Some(t) => {
                            widgets::status_pill(ui, Status::Synced);
                            note(ui, format!("Up to date · checked {}", ago(t.elapsed())));
                            ui.ctx().request_repaint_after(Duration::from_secs(5));
                        }
                        None if !self.backend.has_session() => {
                            note(ui, "Not signed in".into());
                        }
                        None => {
                            note(ui, "Connecting…".into());
                        }
                    },
                }
                if !self.failed.is_empty() {
                    ui.separator();
                    widgets::status_pill(ui, Status::Failed);
                    note(ui, format!("{} transfer(s)", self.failed.len()));
                }
                for (_, info, frac) in &self.transfers {
                    ui.separator();
                    let verb = if info.upload { "Uploading" } else { "Downloading" };
                    note(ui, format!("{verb} {}", info.name));
                    match frac {
                        Some(f) => widgets::progress(ui, *f, 96.0),
                        None => {
                            ui.add(egui::Spinner::new().size(12.0).color(p.ink_muted));
                        }
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !self.selected.is_empty() {
                        note(ui, format!("{} selected", self.selected.len()));
                    }
                    if !self.cut.is_empty() {
                        note(ui, format!("✂ {} to move", self.cut.len()));
                    }
                });
            });
        });
    }

    fn file_table(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let p = theme::of(ui);
        let show_location = self.is_searching() || !matches!(self.view, View::MyDrive | View::Folder(_));
        let in_trash = self.view == View::Trash;
        // Uploads into the folder on screen (running or failed) are listed above its files.
        let here = if self.is_searching() { None } else { self.current_folder() };
        let pending: Vec<Pending> = match &here {
            Some(folder) => self
                .transfers
                .iter()
                .filter(|(_, info, _)| info.upload && info.target == *folder)
                .map(|(_, info, frac)| Pending::Uploading(info, *frac))
                .chain(
                    self.failed
                        .iter()
                        .enumerate()
                        .filter(|(_, (info, _))| info.upload && info.target == *folder)
                        .map(|(i, (info, err))| Pending::Failed(i, info, err)),
                )
                .collect(),
            None => Vec::new(),
        };
        let n_pending = pending.len();
        let n = self.rows.len();
        if n + n_pending == 0 {
            ui.add_space(theme::SPACE_12);
            ui.vertical_centered(|ui| {
                if !self.cache_loaded {
                    ui.add(egui::Spinner::new().color(p.ink_muted));
                } else if self.is_searching() {
                    ui.label(RichText::new("No matches in the local index.").weak());
                    ui.add_space(theme::SPACE_2);
                    if ui.add(Button::secondary("Search file contents on Drive")).clicked() {
                        actions.push(Action::ContentSearch);
                    }
                } else if matches!(self.sync, SyncState::Initial { .. }) {
                    ui.add(egui::Spinner::new().color(p.ink_muted));
                    ui.label(RichText::new("Building local index…").weak());
                } else {
                    ui.label(RichText::new("Empty").weak());
                }
            });
            return;
        }
        // Keep the scroll request alive until the row exists (e.g. a just-uploaded file
        // appears once the background index rebuild lands).
        let scroll_to = if self.scroll_to_cursor { self.cursor_pos() } else { None };
        if scroll_to.is_some() {
            self.scroll_to_cursor = false;
        }
        let row_h = if self.settings.compact_rows { theme::ROW_COMPACT } else { theme::ROW_DEFAULT };
        let sort = self.sort;
        let arrow = |k: SortKey| match sort {
            Some((s, desc)) if s == k => if desc { " ↓" } else { " ↑" },
            _ => "",
        };
        let header_text = |text: String| RichText::new(text).font(theme::semibold_font(theme::FONT_UI_SMALL)).color(p.ink_muted);
        let drop_hover = self.drop_hover.as_ref().map(|h| h.0.clone());
        let mut targets: Vec<DropTarget> = Vec::new();
        let ctrl_held = ui.input(|i| i.modifiers.command);
        let shift_held = ui.input(|i| i.modifiers.shift);
        // Row fills span the whole pane, including its side padding.
        let full_x = ui.max_rect().x_range().expand(theme::SPACE_3);
        let row_bg = |ui: &egui::Ui, selected: bool, drop: bool| {
            let clip = egui::Rect::from_x_y_ranges(full_x, ui.clip_rect().y_range());
            let rect = egui::Rect::from_x_y_ranges(full_x, ui.max_rect().y_range());
            let hovered = ui.ctx().rect_contains_pointer(ui.layer_id(), clip.intersect(rect));
            let mut painter = ui.painter().clone();
            painter.set_clip_rect(clip);
            widgets::paint_row(&painter, rect, selected, hovered, drop);
        };
        ui.scope(|ui| {
            // Rows are exactly ROW_DEFAULT / ROW_COMPACT tall, columns `space-3` apart.
            ui.spacing_mut().item_spacing = egui::vec2(theme::SPACE_3, 0.0);
            // Rows paint their own hover/selection and hairline (widgets::paint_row), and
            // column-resize handles only show on hover.
            ui.visuals_mut().widgets.hovered.bg_fill = egui::Color32::TRANSPARENT;
            ui.visuals_mut().widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
            // Name is the non-resizable `remainder` column: egui_extras only re-fits such
            // columns to the available width, so the listing always fills the window. The
            // other columns stay resizable.
            let mut table = TableBuilder::new(ui)
                .striped(false)
                .resizable(true)
                .sense(Sense::click())
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::remainder().at_least(220.0).clip(true).resizable(false));
            if show_location {
                table = table.column(Column::initial(260.0).at_least(80.0).clip(true));
            }
            table = table
                .column(Column::initial(120.0).at_least(80.0).clip(true))
                .column(Column::initial(120.0).at_least(70.0).clip(true))
                .column(Column::initial(80.0).at_least(50.0));
            if let Some(r) = scroll_to {
                table = table.scroll_to_row(r + n_pending, None);
            }
            table
                .header(theme::ROW_COMPACT, |mut h| {
                    h.col(|ui| {
                        row_bg(ui, false, false);
                        if ui.add(egui::Button::new(header_text(format!("Name{}", arrow(SortKey::Name)))).frame(false)).clicked() {
                            actions.push(Action::Sort(SortKey::Name));
                        }
                    });
                    if show_location {
                        h.col(|ui| {
                            ui.label(header_text("Location".into()));
                        });
                    }
                    h.col(|ui| {
                        ui.label(header_text("Owner".into()));
                    });
                    h.col(|ui| {
                        if ui.add(egui::Button::new(header_text(format!("Modified{}", arrow(SortKey::Modified)))).frame(false)).clicked() {
                            actions.push(Action::Sort(SortKey::Modified));
                        }
                    });
                    h.col(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(theme::SPACE_2); // clear of the floating scroll bar
                            if ui.add(egui::Button::new(header_text(format!("Size{}", arrow(SortKey::Size)))).frame(false)).clicked() {
                                actions.push(Action::Sort(SortKey::Size));
                            }
                        });
                    });
                })
                .body(|body| {
                    body.rows(row_h, n_pending + n, |mut row| {
                        if row.index() < n_pending {
                            let item = &pending[row.index()];
                            let (info, failed) = match item {
                                Pending::Uploading(info, _) => (*info, None),
                                Pending::Failed(i, info, err) => (*info, Some((*i, *err))),
                            };
                            row.col(|ui| {
                                row_bg(ui, false, false);
                                ui.label(RichText::new(Kind::Other.icon()).color(p.ink_muted));
                                ui.add(egui::Label::new(RichText::new(&info.name).color(p.ink)).truncate());
                            });
                            if show_location {
                                row.col(|_| {});
                            }
                            row.col(|ui| match item {
                                Pending::Uploading(_, Some(f)) => widgets::progress(ui, *f, 64.0),
                                Pending::Uploading(_, None) => {
                                    ui.add(egui::Spinner::new().size(12.0).color(p.ink_muted));
                                }
                                Pending::Failed(_, _, err) => {
                                    ui.label(RichText::new("Failed").font(theme::semibold_font(theme::FONT_UI_SMALL)).color(p.danger))
                                        .on_hover_text(*err);
                                }
                            });
                            row.col(|ui| match failed {
                                Some((i, _)) => {
                                    if ui.link(widgets::mono_small("Retry")).clicked() {
                                        actions.push(Action::RetryUpload(i));
                                    }
                                }
                                None => {
                                    ui.label(widgets::mono_small("Uploading").color(p.ink_muted));
                                }
                            });
                            row.col(|ui| {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.add_space(theme::SPACE_2);
                                    ui.label(widgets::mono_small(info.size.map(fmt_size).unwrap_or_else(|| "—".into())).color(p.ink_muted));
                                });
                            });
                            if let Some((i, err)) = failed {
                                row.response().on_hover_text(err).context_menu(|ui| {
                                    ui.set_min_width(200.0);
                                    if ui.button("Retry upload").clicked() {
                                        actions.push(Action::RetryUpload(i));
                                        ui.close();
                                    }
                                    if ui.button("Dismiss").clicked() {
                                        actions.push(Action::DismissFailed(i));
                                        ui.close();
                                    }
                                });
                            }
                            return;
                        }
                        let e = &self.index.entries[self.rows[row.index() - n_pending]];
                        let f = &e.file;
                        let is_sel = self.selected.contains(&f.id);
                        let is_cursor = self.cursor.as_deref() == Some(f.id.as_str());
                        let folder_target = f.folder_target().map(str::to_string);
                        let drop_here = folder_target.is_some() && folder_target == drop_hover;
                        let selected = is_sel || (is_cursor && self.selected.is_empty());
                        let transfer = self.transfers.iter().find(|(_, t, _)| !t.upload && t.target == f.id);
                        let failure = self.failed.iter().find(|(t, _)| !t.upload && t.target == f.id);
                        row.col(|ui| {
                            row_bg(ui, selected, drop_here);
                            let icon_color = if e.kind == Kind::Folder { p.brand } else if selected { p.ink } else { p.ink_muted };
                            ui.label(RichText::new(e.kind.icon()).color(icon_color));
                            let mut name = RichText::new(&f.name).color(p.ink);
                            if self.cut.iter().any(|(id, _)| *id == f.id) {
                                name = name.italics().color(p.ink_muted);
                            }
                            ui.add(egui::Label::new(name).truncate());
                            if f.starred {
                                ui.label(RichText::new("★").color(p.ink_muted));
                            }
                            if f.shared {
                                ui.label(RichText::new("👥").color(p.ink_muted));
                            }
                            if let Some(lc) = self.local.get(&f.id) {
                                if lc.outdated(f) {
                                    ui.label(RichText::new("💾").color(p.warning)).on_hover_text(format!(
                                        "Downloaded to {}\nDrive has a newer version (Ctrl+S to update)",
                                        lc.path.display()
                                    ));
                                } else {
                                    ui.label(RichText::new("💾").color(p.success))
                                        .on_hover_text(format!("Downloaded to {}\nDouble-click opens it", lc.path.display()));
                                }
                            }
                        });
                        if show_location {
                            row.col(|ui| {
                                ui.add(egui::Label::new(widgets::mono_small(&*e.path).color(p.ink_muted)).truncate());
                            });
                        }
                        row.col(|ui| {
                            if let Some((_, _, frac)) = transfer {
                                match frac {
                                    Some(fr) => widgets::progress(ui, *fr, 64.0),
                                    None => {
                                        ui.add(egui::Spinner::new().size(12.0).color(p.ink_muted));
                                    }
                                }
                            } else if let Some((_, err)) = failure {
                                ui.label(RichText::new("Failed").font(theme::semibold_font(theme::FONT_UI_SMALL)).color(p.danger))
                                    .on_hover_text(err);
                            } else {
                                let owner = if f.drive_id.is_some() {
                                    "Shared drive"
                                } else if f.owned_by_me {
                                    "Me"
                                } else {
                                    "Shared with me"
                                };
                                ui.add(egui::Label::new(RichText::new(owner).small().color(p.ink_muted)).truncate());
                            }
                        });
                        row.col(|ui| {
                            let text = if transfer.is_some() {
                                "Downloading".to_string()
                            } else if failure.is_some() {
                                "Not downloaded".to_string()
                            } else {
                                fmt_time(f.modified_time.as_deref())
                            };
                            ui.label(widgets::mono_small(text).color(p.ink_muted));
                        });
                        row.col(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add_space(theme::SPACE_2);
                                let size = f.size_bytes().map(fmt_size).unwrap_or_else(|| "—".into());
                                ui.label(widgets::mono_small(size).color(p.ink_muted));
                            });
                        });
                        let resp = row.response();
                        if let Some(t) = folder_target {
                            targets.push((resp.rect, t, f.name.clone()));
                        }
                        if resp.clicked() {
                            actions.push(Action::Click { id: f.id.clone(), ctrl: ctrl_held, shift: shift_held });
                        }
                        if resp.double_clicked() {
                            actions.push(Action::Open(f.id.clone()));
                        }
                        if resp.secondary_clicked() && !is_sel {
                            actions.push(Action::Click { id: f.id.clone(), ctrl: false, shift: false });
                        }
                        let is_folder = f.folder_target().is_some();
                        let has_local = self.local.get(&f.id).is_some_and(|l| l.path.exists());
                        resp.context_menu(|ui| {
                            ui.set_min_width(240.0);
                            let mut item = |ui: &mut egui::Ui, label: RichText, hint: &str, a: Action| {
                                let b = egui::Button::new(label).shortcut_text(widgets::mono_small(hint).color(p.ink_muted));
                                if ui.add(b).clicked() {
                                    actions.push(a);
                                    ui.close();
                                }
                            };
                            let open_label = if is_folder {
                                "📂 Open"
                            } else if has_local {
                                "↗ Open local copy"
                            } else {
                                "↗ Open"
                            };
                            let t = |s: &str| RichText::new(s).color(p.ink);
                            item(ui, t(open_label), "Enter", Action::Open(f.id.clone()));
                            item(ui, t("🌐 Open in browser"), "Ctrl+O", Action::OpenInBrowser);
                            if has_local {
                                item(ui, t("🗁 Show local file"), "", Action::ShowLocal(f.id.clone()));
                            }
                            if show_location {
                                item(ui, t("📍 Show in folder"), "Ctrl+Enter", Action::Reveal(f.id.clone()));
                            }
                            ui.separator();
                            item(ui, t("🔗 Copy link"), "Ctrl+C", Action::CopyLink);
                            if !is_folder && !f.is_google_native() {
                                item(ui, t("⬇ Copy direct download link"), "Ctrl+Shift+D", Action::CopyDownloadLink);
                            }
                            item(ui, t("🌍 Share: anyone with link + copy"), "Ctrl+Shift+S", Action::Share);
                            item(ui, t("Copy name"), "", Action::CopyName);
                            item(ui, t("Copy ID"), "", Action::CopyId);
                            ui.separator();
                            if !is_folder {
                                item(ui, t(if has_local { "💾 Update local copy" } else { "💾 Download" }), "Ctrl+S", Action::Download);
                            }
                            if has_local {
                                item(ui, t("Forget local copy"), "", Action::ForgetLocal(f.id.clone()));
                            }
                            item(ui, t("✏ Rename"), "F2", Action::Rename);
                            item(ui, t(if f.starred { "☆ Unstar" } else { "★ Star" }), "", Action::Star(!f.starred));
                            item(ui, t("✂ Cut (move)"), "Ctrl+X", Action::Cut);
                            if let Some(t2) = f.folder_target() {
                                item(ui, t("📌 Open this folder at startup"), "", Action::SetStartFolder(t2.to_string()));
                            }
                            if in_trash {
                                item(ui, t("♻ Restore"), "Del", Action::Trash(false));
                            } else {
                                item(ui, RichText::new("🗑 Move to trash").color(p.danger), "Del", Action::Trash(true));
                            }
                        });
                    });
                });
        });
        self.drop_targets.extend(targets);
    }

    /// A modal dialog in the Dray style: `surface-raised`, `radius-lg`, `shadow-popover`,
    /// `space-8` padding.
    fn modal(ctx: &egui::Context, id: &str) -> egui::Modal {
        let p = theme::of_ctx(ctx);
        egui::Modal::new(Id::unique(id))
            .frame(egui::Frame::window(&ctx.global_style()))
            .backdrop_color(p.shadow_popover.color)
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some((id, mut name)) = self.rename.take() {
            let mut keep = true;
            let r = Self::modal(ctx, "rename").show(ctx, |ui| {
                ui.set_width(380.0);
                ui.heading("Rename");
                ui.add_space(theme::SPACE_4);
                let resp = ui.add(text_input(&mut name).desired_width(f32::INFINITY));
                if self.dialog_focus {
                    resp.request_focus();
                    // Select the name without extension.
                    self.dialog_focus = false;
                }
                let enter = ui.input(|i| i.key_pressed(Key::Enter));
                ui.add_space(theme::SPACE_6);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(Button::primary("Rename")).clicked() || enter {
                        if !name.trim().is_empty() {
                            self.backend.rename(id.clone(), name.trim().to_string());
                        }
                        keep = false;
                    }
                    if ui.add(Button::secondary("Cancel")).clicked() {
                        keep = false;
                    }
                });
            });
            if r.should_close() {
                keep = false;
            }
            if keep {
                self.rename = Some((id, name));
            } else {
                self.focus_search = true;
            }
        }
        if let Some(mut name) = self.new_folder.take() {
            let mut keep = true;
            let r = Self::modal(ctx, "newfolder").show(ctx, |ui| {
                ui.set_width(380.0);
                ui.heading("New folder");
                ui.add_space(theme::SPACE_4);
                let resp = ui.add(text_input(&mut name).desired_width(f32::INFINITY));
                if self.dialog_focus {
                    resp.request_focus();
                    self.dialog_focus = false;
                }
                let enter = ui.input(|i| i.key_pressed(Key::Enter));
                ui.add_space(theme::SPACE_6);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(Button::primary("Create")).clicked() || enter {
                        if let (false, Some(parent)) = (name.trim().is_empty(), self.upload_target()) {
                            self.backend.create_folder(name.trim().to_string(), parent);
                        }
                        keep = false;
                    }
                    if ui.add(Button::secondary("Cancel")).clicked() {
                        keep = false;
                    }
                });
            });
            if r.should_close() {
                keep = false;
            }
            if keep {
                self.new_folder = Some(name);
            } else {
                self.focus_search = true;
            }
        }
        if self.about_open {
            let mut close = false;
            let r = Self::modal(ctx, "about").show(ctx, |ui| {
                ui.set_width(360.0);
                ui.vertical_centered(|ui| {
                    ui.heading("Dray");
                    ui.horizontal(|ui| {
                        // Centre the version line.
                        let version = env!("CARGO_PKG_VERSION");
                        let w = 60.0 + version.len() as f32 * 7.2;
                        ui.add_space((ui.available_width() - w).max(0.0) / 2.0);
                        ui.label(RichText::new("Version").small().weak());
                        ui.label(widgets::mono_small(version).weak());
                    });
                    ui.add_space(theme::SPACE_4);
                    ui.label("A fast, native desktop client for Google Drive.");
                    ui.add_space(theme::SPACE_2);
                    ui.label(
                        RichText::new("Dray is an independent project and is not affiliated with or endorsed by Google.")
                            .small()
                            .weak(),
                    );
                    ui.add_space(theme::SPACE_4);
                    ui.label(RichText::new("Monospace type: JetBrains Mono (OFL 1.1).").small().weak());
                    ui.add_space(theme::SPACE_6);
                    if ui.add(Button::secondary("Close")).clicked() {
                        close = true;
                    }
                });
            });
            if close || r.should_close() {
                self.about_open = false;
            }
        }
        if self.settings_open {
            let mut open = true;
            egui::Window::new("Settings")
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                ui.set_width(500.0);
                ui.horizontal(|ui| {
                    ui.heading("Settings");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(Button::ghost("✕")).on_hover_text("Close").clicked() {
                            open = false;
                        }
                    });
                });
                ui.add_space(theme::SPACE_4);
                let mut changed = false;
                widgets::section_label(ui, "General");
                ui.add_space(theme::SPACE_2);
                egui::Grid::new("settings-grid").num_columns(2).spacing([theme::SPACE_4, theme::SPACE_2]).show(ui, |ui| {
                    ui.label(RichText::new("Theme").weak());
                    let mut dm = self.settings.dark_mode;
                    egui::ComboBox::from_id_salt("theme")
                        .selected_text(match dm { None => "System", Some(true) => "Dark", Some(false) => "Light" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut dm, None, "System");
                            ui.selectable_value(&mut dm, Some(false), "Light");
                            ui.selectable_value(&mut dm, Some(true), "Dark");
                        });
                    if dm != self.settings.dark_mode {
                        self.settings.dark_mode = dm;
                        changed = true;
                        match dm {
                            Some(true) => ctx.set_theme(egui::Theme::Dark),
                            Some(false) => ctx.set_theme(egui::Theme::Light),
                            None => ctx.set_theme(egui::ThemePreference::System),
                        }
                    }
                    ui.end_row();

                    ui.label(RichText::new("Row density").weak());
                    let mut compact = self.settings.compact_rows;
                    egui::ComboBox::from_id_salt("density")
                        .selected_text(if compact { "Compact" } else { "Default" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut compact, false, "Default");
                            ui.selectable_value(&mut compact, true, "Compact");
                        });
                    if compact != self.settings.compact_rows {
                        self.settings.compact_rows = compact;
                        changed = true;
                    }
                    ui.end_row();

                    ui.label(RichText::new("Open at startup").weak());
                    ui.horizontal(|ui| {
                        let current = self.settings.start_view.as_deref().and_then(View::from_setting).unwrap_or(View::MyDrive);
                        ui.label(self.view_label(&current));
                        if self.view != current && ui.add(Button::secondary(format!("Use “{}”", self.view_label(&self.view)))).clicked() {
                            self.settings.start_view = Some(self.view.to_setting());
                            changed = true;
                        }
                        if self.settings.start_view.is_some() && ui.add(Button::ghost("Default")).clicked() {
                            self.settings.start_view = None;
                            changed = true;
                        }
                    });
                    ui.end_row();

                    ui.label(RichText::new("Download to").weak());
                    ui.horizontal(|ui| {
                        let dir = self.download_dir();
                        ui.add(egui::Label::new(widgets::mono_small(dir.display().to_string())).truncate())
                            .on_hover_text(dir.display().to_string());
                        if ui.add(Button::secondary("Change…")).clicked() {
                            let tx = self.ui_tx.clone();
                            let ctx = ctx.clone();
                            std::thread::spawn(move || {
                                if let Some(p) = rfd::FileDialog::new().set_directory(&dir).pick_folder() {
                                    let _ = tx.send(UiMsg::DownloadDir(p));
                                    ctx.request_repaint();
                                }
                            });
                        }
                        if self.settings.download_dir.is_some() && ui.add(Button::ghost("Default")).clicked() {
                            self.settings.download_dir = None;
                            changed = true;
                        }
                    });
                    ui.end_row();

                    ui.label(RichText::new("Check for changes every").weak());
                    if ui.add(egui::DragValue::new(&mut self.settings.poll_seconds).range(5..=600).suffix(" s")).changed() {
                        self.backend.set_poll_secs(self.settings.poll_seconds);
                        changed = true;
                    }
                    ui.end_row();

                    ui.label("");
                    changed |= ui.checkbox(&mut self.settings.copy_link_after_upload, "Copy link after uploading a single file").changed();
                    ui.end_row();
                });
                if changed {
                    self.settings.save(&self.paths);
                }
                ui.add_space(theme::SPACE_4);
                ui.separator();
                ui.add_space(theme::SPACE_2);
                widgets::section_label(ui, "Local index");
                ui.add_space(theme::SPACE_2);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme::SPACE_1;
                    ui.label(widgets::mono_small(self.files.len().to_string()));
                    ui.label("items, built in");
                    ui.label(widgets::mono_small(format!("{:.0} ms", self.build_cost.as_secs_f64() * 1000.0)));
                });
                ui.add_space(theme::SPACE_2);
                ui.horizontal(|ui| {
                    if ui.add(Button::secondary("Rebuild cache")).on_hover_text("Re-list everything from Drive").clicked() {
                        self.backend.rebuild_cache();
                        self.toast("Rebuilding local index…", false);
                    }
                    if ui.add(Button::danger("Sign out")).clicked() {
                        self.backend.sign_out();
                        self.settings_open = false;
                    }
                });
                ui.add_space(theme::SPACE_4);
                ui.separator();
                ui.collapsing("Keyboard shortcuts", |ui| {
                    egui::Grid::new("keys").num_columns(2).spacing([theme::SPACE_4, theme::SPACE_2]).show(ui, |ui| {
                        for (k, d) in [
                            ("Ctrl+F / Ctrl+K", "Focus search (or just start typing)"),
                            ("Ctrl+Shift+F", "Search only the open folder (toggle)"),
                            ("↑ ↓  PgUp PgDn", "Move selection (works while typing)"),
                            ("Enter / double-click", "Open (local copy if downloaded) / enter folder"),
                            ("Ctrl+Enter", "Show in folder"),
                            ("Shift+Enter", "Search inside file contents (Drive)"),
                            ("Ctrl+C / Ctrl+Shift+C", "Copy link(s)"),
                            ("Ctrl+Shift+D", "Copy direct download link"),
                            ("Ctrl+Shift+S", "Share with anyone with link + copy"),
                            ("Ctrl+O", "Open in browser"),
                            ("Ctrl+S", "Download (or update the local copy)"),
                            ("Ctrl+U / drag & drop", "Upload (drop onto a folder to upload into it)"),
                            ("Ctrl+Shift+N", "New folder"),
                            ("F2", "Rename"),
                            ("Delete", "Move to trash / restore"),
                            ("Ctrl+X, Ctrl+V", "Move items to another folder"),
                            ("Backspace / Alt+↑", "Parent folder"),
                            ("Alt+← / Alt+→", "Back / forward"),
                            ("Esc", "Clear search"),
                            ("F5 / Ctrl+R", "Sync now"),
                        ] {
                            widgets::key_hint(ui, k);
                            ui.label(d);
                            ui.end_row();
                        }
                    });
                });
            });
            if !open {
                self.settings_open = false;
            }
        }
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.toasts.retain(|t| t.until > now);
        if self.toasts.is_empty() {
            return;
        }
        let next = self.toasts.iter().map(|t| t.until).min().unwrap();
        ctx.request_repaint_after(next - now);
        egui::Area::new(Id::unique("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-theme::SPACE_4, -(theme::SPACE_12 + theme::SPACE_1)))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                ui.with_layout(Layout::bottom_up(Align::Max), |ui| {
                    ui.spacing_mut().item_spacing.y = theme::SPACE_2;
                    for t in self.toasts.iter().rev() {
                        egui::Frame::popup(ui.style())
                            .inner_margin(egui::Margin::symmetric(theme::SPACE_3 as i8, theme::SPACE_2 as i8))
                            .show(ui, |ui| {
                                ui.set_max_width(460.0);
                                ui.horizontal(|ui| {
                                    if t.error {
                                        widgets::status_pill(ui, Status::Failed);
                                    }
                                    ui.add(egui::Label::new(RichText::new(&t.text).color(theme::of(ui).ink)).wrap());
                                });
                            });
                    }
                });
            });
    }

    /// Drag & drop upload. Dropping onto a folder row, sidebar entry or breadcrumb uploads
    /// into that folder; anywhere else uploads into the folder being viewed.
    fn handle_drops(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let (hovering, dropped) = ctx.input(|i| {
            (
                !i.raw.hovered_files.is_empty(),
                i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect::<Vec<_>>(),
            )
        });
        if !hovering && dropped.is_empty() {
            self.drop_hover = None;
            return;
        }
        // The drag source owns the pointer, so ask the window system where it is.
        let probe = self.probe.get_or_insert_with(|| PointerProbe::new(frame));
        let ppp = ctx.pixels_per_point();
        let pos = probe
            .position()
            .map(|(x, y)| egui::pos2(x / ppp, y / ppp))
            .or_else(|| ctx.input(|i| i.pointer.latest_pos()));
        let hit = pos.and_then(|p| {
            // Later targets (rows) are drawn on top of earlier ones; prefer the smallest hit.
            self.drop_targets
                .iter()
                .filter(|(r, _, _)| r.contains(p))
                .min_by(|a, b| a.0.area().total_cmp(&b.0.area()))
                .map(|(r, id, name)| (*r, id.clone(), name.clone()))
        });
        self.drop_hover = hit.as_ref().map(|(_, id, name)| (id.clone(), name.clone()));
        let fallback = || {
            let id = self.upload_target()?;
            let name = if Some(&id) == self.my_drive.as_ref() {
                "My Drive".to_string()
            } else {
                self.drive_names
                    .get(&id)
                    .cloned()
                    .or_else(|| self.files.get(&id).map(|f| f.name.clone()))
                    .unwrap_or_else(|| "this folder".into())
            };
            Some((id, name))
        };
        let target = self.drop_hover.clone().or_else(fallback);

        if hovering {
            ctx.request_repaint_after(Duration::from_millis(30)); // no pointer events while dragging
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::unique("drop")));
            let screen = ctx.content_rect();
            let p = theme::of_ctx(ctx);
            let accent = p.brand;
            match &hit {
                Some((rect, _, _)) => {
                    painter.rect_filled(*rect, theme::RADIUS_MD, p.brand_subtle);
                    painter.rect_stroke(*rect, theme::RADIUS_MD, egui::Stroke::new(2.0, accent), egui::StrokeKind::Inside);
                }
                None => {
                    painter.rect_filled(screen, 0.0, p.brand_subtle.gamma_multiply(0.5));
                    painter.rect_stroke(screen.shrink(3.0), theme::RADIUS_LG, egui::Stroke::new(2.0, accent), egui::StrokeKind::Inside);
                }
            }
            let text = match &target {
                Some((_, name)) => format!("⬆  Drop to upload into “{name}”"),
                None => "Still syncing — can't upload yet".into(),
            };
            let galley = painter.layout_no_wrap(text, theme::semibold_font(theme::FONT_UI_TITLE), p.on_brand);
            let pill = egui::Rect::from_center_size(
                egui::pos2(screen.center().x, screen.bottom() - 60.0),
                galley.size() + egui::vec2(32.0, 18.0),
            );
            painter.rect_filled(pill, theme::RADIUS_LG, accent);
            painter.galley(pill.center() - galley.size() / 2.0, galley, p.on_brand);
        }
        if !dropped.is_empty() {
            self.drop_hover = None;
            match target {
                Some((parent, name)) => {
                    self.toast(format!("Uploading {} item(s) into “{name}”…", dropped.len()), false);
                    self.backend.upload(dropped, parent);
                }
                None => self.toast("Still syncing — try again in a moment", true),
            }
        }
    }
}

impl eframe::App for App {
    /// Runs every pass, even when the compositor isn't asking for frames (hidden window), so
    /// sync events and index rebuilds keep flowing in the background.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events(ctx);
        if matches!(self.screen, Screen::Main) {
            self.maybe_rebuild(ctx);
        }
        if self.building.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if matches!(self.screen, Screen::Setup) {
            self.setup_ui(ui);
            return;
        }
        self.handle_keys(&ctx);
        self.recompute_rows();
        self.handle_drops(&ctx, frame);

        let mut actions = Vec::new();
        self.drop_targets.clear();
        self.status_bar(ui);
        self.sidebar(ui, &mut actions);
        self.top_bar(ui, &mut actions);
        let p = theme::of(ui);
        let list = egui::Frame::new().fill(p.surface_raised).inner_margin(egui::Margin::symmetric(theme::SPACE_3 as i8, 0));
        egui::CentralPanel::default().frame(list).show(ui, |ui| {
            self.file_table(ui, &mut actions);
        });
        self.dialogs(&ctx);
        self.toasts_ui(&ctx);
        ctx.input(|i| {
            if i.key_pressed(Key::Tab) {
                self.keyboard_focus = true;
            }
            if i.pointer.any_pressed() {
                self.keyboard_focus = false;
            }
        });
        widgets::focus_ring(&ctx, self.keyboard_focus);
        for a in actions {
            self.apply(&ctx, a);
        }
        // Searching may have changed after actions; next frame picks it up.
        if self.rows_dirty || self.query != self.last_query {
            ctx.request_repaint();
        }
    }
}

/// Single-line input: 28 px tall with the control padding.
fn text_input(text: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text).margin(egui::vec2(theme::SPACE_2, 5.0))
}

fn search_id() -> Id {
    Id::unique("search-box")
}

fn link_for(f: &DFile) -> String {
    if f.is_folder() {
        format!("https://drive.google.com/drive/folders/{}", f.id)
    } else {
        f.view_link()
    }
}

fn fmt_size(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else if v < 10.0 { format!("{v:.1} {}", U[i]) } else { format!("{v:.0} {}", U[i]) }
}

fn fmt_time(t: Option<&str>) -> String {
    use chrono::{DateTime, Datelike, Local};
    let Some(dt) = t.and_then(|s| DateTime::parse_from_rfc3339(s).ok()) else { return "—".into() };
    let dt = dt.with_timezone(&Local);
    let now = Local::now();
    if dt.date_naive() == now.date_naive() {
        dt.format("%H:%M").to_string()
    } else if dt.year() == now.year() {
        dt.format("%b %e").to_string()
    } else {
        dt.format("%Y-%m-%d").to_string()
    }
}

fn ago(d: Duration) -> String {
    let s = d.as_secs();
    if s < 5 {
        "just now".into()
    } else if s < 60 {
        format!("{s}s ago")
    } else {
        format!("{}m ago", s / 60)
    }
}

/// Show a file in the system file manager, selected if the platform supports it.
fn reveal_in_file_manager(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
        return;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // freedesktop FileManager1 (Dolphin, Nautilus, Nemo, …) highlights the file.
        let uri = reqwest::Url::from_file_path(path).map(|u| u.to_string()).unwrap_or_default();
        let ok = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(format!("array:string:{uri}"))
            .arg("string:")
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            if let Some(dir) = path.parent() {
                let _ = open::that_detached(dir);
            }
        }
    }
}
