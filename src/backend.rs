//! Background work: cache loading, sync (full + incremental via the Changes API) and file
//! operations. Everything here runs off the UI thread and reports back through [`Event`]s.

use crate::auth::{Auth, AuthRevoked};
use crate::cache::Cache;
use crate::config::{ClientCreds, Paths};
use crate::drive::{ApiError, Drive};
use crate::model::{DFile, SharedDrive};
use anyhow::Result;
use chrono::Datelike;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

const LIST_THREADS: usize = 8;
const UPLOAD_THREADS: usize = 4;

#[derive(Debug, Clone)]
pub enum SyncState {
    Idle,
    Initial { files: usize },
    Incremental,
    Offline(String),
}

pub enum Event {
    CacheLoaded(Vec<DFile>),
    Upsert(Vec<DFile>),
    Remove(Vec<String>),
    ResetFiles,
    Roots { my_drive: Option<String>, drives: Vec<SharedDrive> },
    Sync(SyncState),
    SyncedAt(Instant),
    LoggedIn,
    AuthRequired(String),
    Toast { text: String, error: bool },
    Progress { id: u64, label: String, frac: Option<f32> },
    ProgressDone(u64),
    Uploaded { file: DFile, single: bool },
    ContentResults { query: String, ids: Vec<String> },
    CopyText { text: String, toast: String },
    LocalCopies(Vec<(String, PathBuf, String)>),
    Downloaded { id: String, path: PathBuf, modified: String },
}

enum SyncCmd {
    Now,
    Full,
}

#[derive(Clone)]
pub struct Backend {
    inner: Arc<Inner>,
}

struct Inner {
    paths: Paths,
    http: reqwest::blocking::Client,
    drive: RwLock<Option<Drive>>,
    cache: Mutex<Cache>,
    tx: Mutex<Sender<Event>>,
    ctx: eframe::egui::Context,
    wake: Mutex<Sender<SyncCmd>>,
    next_id: AtomicU64,
    poll_secs: AtomicU64,
    login_running: AtomicBool,
}

impl Backend {
    pub fn new(ctx: eframe::egui::Context, paths: Paths, poll_secs: u64) -> Result<(Backend, Receiver<Event>)> {
        let http = reqwest::blocking::Client::builder()
            .user_agent("dray/0.1 (gzip)")
            .gzip(true)
            .pool_max_idle_per_host(16)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .build()?;
        let cache = Cache::open(&paths.cache_file())?;
        let (tx, rx) = channel();
        let (wake_tx, wake_rx) = channel();
        let inner = Arc::new(Inner {
            paths,
            http,
            drive: RwLock::new(None),
            cache: Mutex::new(cache),
            tx: Mutex::new(tx),
            ctx,
            wake: Mutex::new(wake_tx),
            next_id: AtomicU64::new(1),
            poll_secs: AtomicU64::new(poll_secs.max(5)),
            login_running: AtomicBool::new(false),
        });
        let b = Backend { inner };

        // Restore a saved session.
        if let Some(creds) = ClientCreds::load(&b.inner.paths) {
            if let Some(auth) = Auth::load(creds, b.inner.http.clone(), b.inner.paths.token_file()) {
                *b.inner.drive.write().unwrap() = Some(Drive::new(b.inner.http.clone(), Arc::new(auth)));
            }
        }

        let bb = b.clone();
        std::thread::Builder::new()
            .name("sync".into())
            .spawn(move || bb.sync_loop(wake_rx))?;
        Ok((b, rx))
    }

    pub fn has_session(&self) -> bool {
        self.inner.drive.read().unwrap().is_some()
    }

    fn drive(&self) -> Option<Drive> {
        self.inner.drive.read().unwrap().clone()
    }

    fn send(&self, e: Event) {
        let _ = self.inner.tx.lock().unwrap().send(e);
        self.inner.ctx.request_repaint();
    }

    fn toast_err(&self, what: &str, e: &anyhow::Error) {
        self.send(Event::Toast { text: format!("{what}: {e:#}"), error: true });
    }

    pub fn sync_now(&self) {
        let _ = self.inner.wake.lock().unwrap().send(SyncCmd::Now);
    }

    pub fn rebuild_cache(&self) {
        let _ = self.inner.wake.lock().unwrap().send(SyncCmd::Full);
    }

    pub fn set_poll_secs(&self, s: u64) {
        self.inner.poll_secs.store(s.max(5), Ordering::Relaxed);
    }

    // ---------------------------------------------------------------- session

    pub fn login(&self, creds: ClientCreds) {
        if self.inner.login_running.swap(true, Ordering::SeqCst) {
            // A previous attempt is still waiting on the browser; just reopen it by starting anew
            // would leave a dangling listener, so ignore duplicate clicks.
            return;
        }
        let b = self.clone();
        std::thread::spawn(move || {
            let res = creds
                .save(&b.inner.paths)
                .and_then(|_| Auth::login(creds, b.inner.http.clone(), b.inner.paths.token_file()));
            b.inner.login_running.store(false, Ordering::SeqCst);
            match res {
                Ok(auth) => {
                    *b.inner.drive.write().unwrap() = Some(Drive::new(b.inner.http.clone(), Arc::new(auth)));
                    b.send(Event::LoggedIn);
                    b.sync_now();
                }
                Err(e) => b.send(Event::AuthRequired(format!("{e:#}"))),
            }
        });
    }

    pub fn sign_out(&self) {
        *self.inner.drive.write().unwrap() = None;
        let _ = std::fs::remove_file(self.inner.paths.token_file());
        let _ = self.inner.cache.lock().unwrap().clear();
        self.send(Event::ResetFiles);
        self.send(Event::Roots { my_drive: None, drives: vec![] });
        self.send(Event::AuthRequired(String::new()));
    }

    // ---------------------------------------------------------------- sync

    fn sync_loop(&self, wake: Receiver<SyncCmd>) {
        // 1. Load the local cache so the UI is useful immediately (even offline).
        {
            let cache = self.inner.cache.lock().unwrap();
            let my_drive = cache.meta("root_id");
            let drives: Vec<SharedDrive> = cache
                .meta("drives")
                .and_then(|j| serde_json::from_str::<Vec<(String, String)>>(&j).ok())
                .unwrap_or_default()
                .into_iter()
                .map(|(id, name)| SharedDrive { id, name })
                .collect();
            let files = cache.load_all().unwrap_or_default();
            let local = cache.local_copies().unwrap_or_default();
            drop(cache);
            self.send(Event::LocalCopies(
                local.into_iter().map(|(id, p, m)| (id, PathBuf::from(p), m)).collect(),
            ));
            self.send(Event::Roots { my_drive, drives });
            self.send(Event::CacheLoaded(files));
        }

        let mut force_full = false;
        loop {
            if let Some(drive) = self.drive() {
                let started = Instant::now();
                let res = self.sync_once(&drive, force_full);
                force_full = false;
                match res {
                    Ok(()) => {
                        self.send(Event::Sync(SyncState::Idle));
                        self.send(Event::SyncedAt(started));
                    }
                    Err(e) if e.downcast_ref::<AuthRevoked>().is_some() => {
                        *self.inner.drive.write().unwrap() = None;
                        self.send(Event::AuthRequired(e.to_string()));
                    }
                    Err(e) => self.send(Event::Sync(SyncState::Offline(format!("{e:#}")))),
                }
            }
            let poll = Duration::from_secs(self.inner.poll_secs.load(Ordering::Relaxed));
            match wake.recv_timeout(poll) {
                Ok(SyncCmd::Full) => force_full = true,
                Ok(SyncCmd::Now) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            // Coalesce bursts of wake-ups.
            while let Ok(cmd) = wake.try_recv() {
                if matches!(cmd, SyncCmd::Full) {
                    force_full = true;
                }
            }
        }
    }

    fn sync_once(&self, drive: &Drive, force_full: bool) -> Result<()> {
        let token = if force_full { None } else { self.inner.cache.lock().unwrap().meta("page_token") };
        match token {
            Some(token) => match self.incremental(drive, &token) {
                Err(e) if matches!(e.downcast_ref::<ApiError>(), Some(a) if a.status.is_client_error()
                    && a.status.as_u16() != 401 && a.status.as_u16() != 403 && a.status.as_u16() != 429) =>
                {
                    self.full_sync(drive)
                }
                r => r,
            },
            None => self.full_sync(drive),
        }
    }

    fn incremental(&self, drive: &Drive, token: &str) -> Result<()> {
        let (changes, next) = drive.changes(token)?;
        if !changes.is_empty() {
            self.send(Event::Sync(SyncState::Incremental));
            let mut up = Vec::new();
            let mut rm = Vec::new();
            for c in changes {
                match c.file {
                    Some(f) if !c.removed => up.push(f),
                    _ => rm.push(c.file_id),
                }
            }
            let mut cache = self.inner.cache.lock().unwrap();
            cache.upsert(&up)?;
            cache.remove(&rm)?;
            drop(cache);
            if !up.is_empty() {
                self.send(Event::Upsert(up));
            }
            if !rm.is_empty() {
                self.send(Event::Remove(rm));
            }
        }
        self.inner.cache.lock().unwrap().set_meta("page_token", &next)?;
        Ok(())
    }

    /// Full listing, parallelised by splitting the corpus into modifiedTime ranges.
    fn full_sync(&self, drive: &Drive) -> Result<()> {
        self.send(Event::Sync(SyncState::Initial { files: 0 }));
        // Take the change token *first* so nothing that changes during listing is lost.
        let token = drive.start_page_token()?;
        let root = drive.root_id()?;
        let drives = drive.shared_drives().unwrap_or_default();
        {
            let cache = self.inner.cache.lock().unwrap();
            cache.set_meta("root_id", &root)?;
            let pairs: Vec<(String, String)> = drives.iter().map(|d| (d.id.clone(), d.name.clone())).collect();
            cache.set_meta("drives", &serde_json::to_string(&pairs)?)?;
        }
        self.send(Event::Roots { my_drive: Some(root), drives: drives.clone() });

        let existing: HashSet<String> = self.inner.cache.lock().unwrap().ids()?.into_iter().collect();

        let mut jobs: VecDeque<(Option<String>, Option<String>)> = VecDeque::new();
        for q in time_buckets() {
            jobs.push_back((Some(q), None));
        }
        for d in &drives {
            jobs.push_back((None, Some(d.id.clone())));
        }
        let jobs = Mutex::new(jobs);
        let seen = Mutex::new(HashSet::<String>::new());
        let count = AtomicU64::new(0);
        let error: Mutex<Option<anyhow::Error>> = Mutex::new(None);

        std::thread::scope(|s| {
            for _ in 0..LIST_THREADS {
                s.spawn(|| {
                    loop {
                        if error.lock().unwrap().is_some() {
                            return;
                        }
                        let Some((q, d)) = jobs.lock().unwrap().pop_front() else { return };
                        let r = drive.list_files(q.as_deref(), d.as_deref(), |page| {
                            if page.is_empty() {
                                return;
                            }
                            let _ = self.inner.cache.lock().unwrap().upsert(&page);
                            seen.lock().unwrap().extend(page.iter().map(|f| f.id.clone()));
                            let n = count.fetch_add(page.len() as u64, Ordering::Relaxed) + page.len() as u64;
                            self.send(Event::Upsert(page));
                            self.send(Event::Sync(SyncState::Initial { files: n as usize }));
                        });
                        if let Err(e) = r {
                            *error.lock().unwrap() = Some(e);
                            return;
                        }
                    }
                });
            }
        });
        if let Some(e) = error.into_inner().unwrap() {
            return Err(e);
        }
        let seen = seen.into_inner().unwrap();
        let stale: Vec<String> = existing.into_iter().filter(|id| !seen.contains(id)).collect();
        if !stale.is_empty() {
            self.inner.cache.lock().unwrap().remove(&stale)?;
            self.send(Event::Remove(stale));
        }
        self.inner.cache.lock().unwrap().set_meta("page_token", &token)?;
        Ok(())
    }

    // ---------------------------------------------------------------- operations

    /// Store updated file metadata locally and push it to the UI immediately.
    fn apply(&self, files: Vec<DFile>) {
        let _ = self.inner.cache.lock().unwrap().upsert(&files);
        self.send(Event::Upsert(files));
    }

    fn op(&self, what: &'static str, f: impl FnOnce(&Backend, &Drive) -> Result<()> + Send + 'static) {
        let Some(drive) = self.drive() else {
            self.send(Event::Toast { text: "Not signed in".into(), error: true });
            return;
        };
        let b = self.clone();
        std::thread::spawn(move || {
            if let Err(e) = f(&b, &drive) {
                b.toast_err(what, &e);
            }
        });
    }

    pub fn rename(&self, id: String, name: String) {
        self.op("Rename failed", move |b, d| {
            b.apply(vec![d.rename(&id, &name)?]);
            Ok(())
        });
    }

    pub fn set_starred(&self, ids: Vec<String>, starred: bool) {
        self.op("Star failed", move |b, d| {
            for id in ids {
                b.apply(vec![d.set_starred(&id, starred)?]);
            }
            Ok(())
        });
    }

    pub fn set_trashed(&self, ids: Vec<String>, trashed: bool) {
        self.op(if trashed { "Trash failed" } else { "Restore failed" }, move |b, d| {
            let n = ids.len();
            for id in ids {
                b.apply(vec![d.set_trashed(&id, trashed)?]);
            }
            let verb = if trashed { "Moved to trash" } else { "Restored" };
            b.send(Event::Toast { text: format!("{verb}: {n} item(s)"), error: false });
            Ok(())
        });
    }

    pub fn move_to(&self, items: Vec<(String, Vec<String>)>, dest: String) {
        self.op("Move failed", move |b, d| {
            let n = items.len();
            for (id, old) in items {
                b.apply(vec![d.move_to(&id, &dest, &old)?]);
            }
            b.send(Event::Toast { text: format!("Moved {n} item(s)"), error: false });
            Ok(())
        });
    }

    pub fn create_folder(&self, name: String, parent: String) {
        self.op("Create folder failed", move |b, d| {
            let f = d.create_folder(&name, &parent)?;
            b.apply(vec![f.clone()]);
            b.send(Event::Uploaded { file: f, single: false });
            Ok(())
        });
    }

    pub fn share_and_copy(&self, file: DFile) {
        self.op("Sharing failed", move |b, d| {
            d.share_anyone(&file.id)?;
            let f = d.get_file(&file.id).unwrap_or(file);
            b.send(Event::CopyText { text: f.view_link(), toast: "Shared (anyone with link) — link copied".into() });
            b.apply(vec![f]);
            Ok(())
        });
    }

    pub fn content_search(&self, query: String) {
        self.op("Content search failed", move |b, d| {
            let files = d.search_content(&query)?;
            let ids = files.iter().map(|f| f.id.clone()).collect();
            b.apply(files);
            b.send(Event::ContentResults { query, ids });
            Ok(())
        });
    }

    pub fn default_download_dir() -> PathBuf {
        directories::UserDirs::new()
            .and_then(|u| u.download_dir().map(|p| p.to_path_buf()))
            .or_else(|| directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// Download files into `dir`. Files with an existing local copy (`Some(path)`) are refreshed
    /// in place. Every successful download is remembered so the app can open it locally.
    pub fn download(&self, files: Vec<(DFile, Option<PathBuf>)>, dir: PathBuf) {
        self.op("Download failed", move |b, d| {
            for (f, existing) in files {
                if f.is_folder() {
                    b.send(Event::Toast { text: format!("Skipping folder '{}'", f.name), error: true });
                    continue;
                }
                let id = b.inner.next_id.fetch_add(1, Ordering::Relaxed);
                let label = format!("⬇ {}", f.name);
                let mut last = Instant::now() - Duration::from_secs(1);
                let r = d.download(&f, &dir, existing.as_deref(), |done, total| {
                    if last.elapsed() > Duration::from_millis(100) {
                        last = Instant::now();
                        let frac = total.map(|t| done as f32 / t.max(1) as f32);
                        b.send(Event::Progress { id, label: label.clone(), frac });
                    }
                });
                b.send(Event::ProgressDone(id));
                let path = r?;
                let modified = f.modified_time.clone().unwrap_or_default();
                let _ = b.inner.cache.lock().unwrap().set_local_copy(&f.id, &path.to_string_lossy(), &modified);
                b.send(Event::Downloaded { id: f.id.clone(), path, modified });
            }
            Ok(())
        });
    }

    pub fn forget_local_copy(&self, id: &str) {
        let _ = self.inner.cache.lock().unwrap().forget_local_copy(id);
    }

    /// Upload files and/or directories (recursively) into `parent`.
    pub fn upload(&self, paths: Vec<PathBuf>, parent: String) {
        self.op("Upload failed", move |b, d| {
            // Create folder structure first, collecting (file, parent) jobs.
            let mut jobs: VecDeque<(PathBuf, String)> = VecDeque::new();
            let mut stack: Vec<(PathBuf, String)> = paths.into_iter().map(|p| (p, parent.clone())).collect();
            let top_level_single = stack.len() == 1 && stack[0].0.is_file();
            while let Some((p, par)) = stack.pop() {
                if p.is_dir() {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("folder").to_string();
                    let folder = d.create_folder(&name, &par)?;
                    let fid = folder.id.clone();
                    b.apply(vec![folder]);
                    for entry in std::fs::read_dir(&p)?.flatten() {
                        stack.push((entry.path(), fid.clone()));
                    }
                } else if p.is_file() {
                    jobs.push_back((p, par));
                }
            }
            let total = jobs.len();
            let jobs = Mutex::new(jobs);
            let errors = Mutex::new(Vec::<String>::new());
            let last_file: Mutex<Option<DFile>> = Mutex::new(None);
            std::thread::scope(|s| {
                for _ in 0..UPLOAD_THREADS.min(total.max(1)) {
                    s.spawn(|| {
                        while let Some((p, par)) = { jobs.lock().unwrap().pop_front() } {
                            let id = b.inner.next_id.fetch_add(1, Ordering::Relaxed);
                            let label = format!("⬆ {}", p.file_name().and_then(|n| n.to_str()).unwrap_or("?"));
                            b.send(Event::Progress { id, label: label.clone(), frac: Some(0.0) });
                            let mut last = Instant::now();
                            let r = d.upload(&p, &par, |sent, len| {
                                if last.elapsed() > Duration::from_millis(100) {
                                    last = Instant::now();
                                    b.send(Event::Progress {
                                        id,
                                        label: label.clone(),
                                        frac: Some(sent as f32 / len.max(1) as f32),
                                    });
                                }
                            });
                            b.send(Event::ProgressDone(id));
                            match r {
                                Ok(f) => {
                                    b.apply(vec![f.clone()]);
                                    *last_file.lock().unwrap() = Some(f);
                                }
                                Err(e) => errors.lock().unwrap().push(format!("{}: {e:#}", p.display())),
                            }
                        }
                    });
                }
            });
            let errors = errors.into_inner().unwrap();
            if let Some(f) = last_file.into_inner().unwrap() {
                b.send(Event::Uploaded { file: f, single: top_level_single && errors.is_empty() });
            }
            if total > 1 || !errors.is_empty() {
                b.send(Event::Toast {
                    text: format!("Uploaded {}/{} file(s)", total - errors.len(), total),
                    error: !errors.is_empty(),
                });
            }
            for e in errors.iter().take(3) {
                b.send(Event::Toast { text: e.clone(), error: true });
            }
            Ok(())
        });
    }
}

/// modifiedTime ranges used to split the initial listing into parallel jobs. Older years are
/// grouped coarsely; the last two years (where most files usually live) are split by month.
fn time_buckets() -> Vec<String> {
    let now = chrono::Utc::now();
    let y = now.year();
    let mut bounds: Vec<String> = Vec::new();
    for year in [2008, 2012, 2015, 2017, 2019] {
        if year < y - 1 {
            bounds.push(format!("{year}-01-01T00:00:00"));
        }
    }
    for year in 2020..(y - 1) {
        bounds.push(format!("{year}-01-01T00:00:00"));
        bounds.push(format!("{year}-07-01T00:00:00"));
    }
    for year in (y - 1)..=y {
        for m in 1..=12 {
            bounds.push(format!("{year}-{m:02}-01T00:00:00"));
        }
    }
    let mut out = Vec::new();
    out.push(format!("modifiedTime < '{}'", bounds[0]));
    for w in bounds.windows(2) {
        out.push(format!("modifiedTime >= '{}' and modifiedTime < '{}'", w[0], w[1]));
    }
    out.push(format!("modifiedTime >= '{}'", bounds.last().unwrap()));
    out
}
