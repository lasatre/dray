//! Thin blocking client for the Drive v3 REST API.

use crate::auth::Auth;
use crate::model::{DFile, FILE_FIELDS, FOLDER_MIME, SharedDrive};
use anyhow::{Context, Result, anyhow, bail};
use reqwest::StatusCode;
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::Deserialize;
use serde_json::json;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const API: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD_API: &str = "https://www.googleapis.com/upload/drive/v3";
/// Files up to this size use a single multipart request (one round trip).
const SIMPLE_UPLOAD_MAX: u64 = 8 * 1024 * 1024;
const CHUNK: usize = 16 * 1024 * 1024; // must be a multiple of 256 KiB

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Drive API error {}: {}", self.status.as_u16(), self.message)
    }
}
impl std::error::Error for ApiError {}

fn api_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.chars().take(300).collect())
}

pub struct Change {
    pub file_id: String,
    pub removed: bool,
    pub file: Option<DFile>,
}

#[derive(Clone)]
pub struct Drive {
    http: Client,
    /// Client that never follows redirects: resumable-upload chunks answer `308 Resume
    /// Incomplete`, which must be handed back to us rather than treated as a redirect.
    upload_http: Client,
    auth: Arc<Auth>,
}

impl Drive {
    pub fn new(http: Client, auth: Arc<Auth>) -> Drive {
        let upload_http = Client::builder()
            .user_agent("dray/0.1")
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| http.clone());
        Drive { http, upload_http, auth }
    }

    /// Execute a request with auth, retrying on 401 (once), rate limits and 5xx with backoff.
    fn exec(&self, mk: impl Fn(&Client) -> RequestBuilder) -> Result<Response> {
        self.exec_with(&self.http, mk)
    }

    fn exec_with(&self, client: &Client, mk: impl Fn(&Client) -> RequestBuilder) -> Result<Response> {
        let mut delay = Duration::from_millis(400);
        let mut refreshed = false;
        let mut attempt = 0;
        loop {
            attempt += 1;
            let token = self.auth.access_token()?;
            let res = mk(client).bearer_auth(&token).send();
            let resp = match res {
                Ok(r) => r,
                Err(e) if attempt < 5 && (e.is_timeout() || e.is_connect() || e.is_request()) => {
                    std::thread::sleep(delay);
                    delay *= 2;
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            let status = resp.status();
            if status.is_success() || status == StatusCode::PERMANENT_REDIRECT {
                return Ok(resp);
            }
            if status == StatusCode::UNAUTHORIZED && !refreshed {
                self.auth.invalidate(&token);
                refreshed = true;
                continue;
            }
            let body = resp.text().unwrap_or_default();
            let rate_limited = status == StatusCode::TOO_MANY_REQUESTS
                || status.is_server_error()
                || (status == StatusCode::FORBIDDEN
                    && (body.contains("rateLimitExceeded") || body.contains("userRateLimitExceeded")));
            if rate_limited && attempt < 6 {
                std::thread::sleep(delay);
                delay *= 2;
                continue;
            }
            return Err(ApiError { status, message: api_message(&body) }.into());
        }
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str, query: &[(&str, &str)]) -> Result<T> {
        Ok(self.exec(|c| c.get(url).query(query))?.json()?)
    }

    pub fn root_id(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct R {
            id: String,
        }
        let r: R = self.get_json(&format!("{API}/files/root"), &[("fields", "id")])?;
        Ok(r.id)
    }

    pub fn shared_drives(&self) -> Result<Vec<SharedDrive>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct R {
            #[serde(default)]
            drives: Vec<SharedDrive>,
            next_page_token: Option<String>,
        }
        let mut out = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut q = vec![("pageSize", "100"), ("fields", "nextPageToken,drives(id,name)")];
            if let Some(p) = &page {
                q.push(("pageToken", p));
            }
            let r: R = self.get_json(&format!("{API}/drives"), &q)?;
            out.extend(r.drives);
            match r.next_page_token {
                Some(p) => page = Some(p),
                None => return Ok(out),
            }
        }
    }

    pub fn start_page_token(&self) -> Result<String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct R {
            start_page_token: String,
        }
        let r: R = self.get_json(
            &format!("{API}/changes/startPageToken"),
            &[("supportsAllDrives", "true")],
        )?;
        Ok(r.start_page_token)
    }

    /// List all files matching `q` in a corpus, calling `on_page` for every page as it arrives.
    /// `drive_id = None` lists the user's corpus (My Drive + shared with me).
    pub fn list_files(
        &self,
        q: Option<&str>,
        drive_id: Option<&str>,
        mut on_page: impl FnMut(Vec<DFile>),
    ) -> Result<()> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct R {
            #[serde(default)]
            files: Vec<DFile>,
            next_page_token: Option<String>,
        }
        let fields = format!("nextPageToken,files({FILE_FIELDS})");
        let mut page: Option<String> = None;
        loop {
            let mut query: Vec<(&str, &str)> = vec![
                ("pageSize", "1000"),
                ("fields", &fields),
                ("supportsAllDrives", "true"),
                ("includeItemsFromAllDrives", "true"),
            ];
            match drive_id {
                Some(d) => {
                    query.push(("corpora", "drive"));
                    query.push(("driveId", d));
                }
                None => query.push(("corpora", "user")),
            }
            if let Some(q) = q {
                query.push(("q", q));
            }
            if let Some(p) = &page {
                query.push(("pageToken", p));
            }
            let r: R = self.get_json(&format!("{API}/files"), &query)?;
            on_page(r.files);
            match r.next_page_token {
                Some(p) => page = Some(p),
                None => return Ok(()),
            }
        }
    }

    /// Fetch all changes since `token`. Returns the changes and the next start token.
    pub fn changes(&self, token: &str) -> Result<(Vec<Change>, String)> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct C {
            #[serde(default)]
            removed: bool,
            file_id: Option<String>,
            file: Option<DFile>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct R {
            #[serde(default)]
            changes: Vec<C>,
            next_page_token: Option<String>,
            new_start_page_token: Option<String>,
        }
        let fields = format!("nextPageToken,newStartPageToken,changes(removed,fileId,file({FILE_FIELDS}))");
        let mut out = Vec::new();
        let mut page = token.to_string();
        loop {
            let r: R = self.get_json(
                &format!("{API}/changes"),
                &[
                    ("pageToken", page.as_str()),
                    ("pageSize", "1000"),
                    ("fields", &fields),
                    ("includeRemoved", "true"),
                    ("supportsAllDrives", "true"),
                    ("includeItemsFromAllDrives", "true"),
                ],
            )?;
            for c in r.changes {
                if let Some(id) = c.file_id {
                    out.push(Change { file_id: id, removed: c.removed, file: c.file });
                }
            }
            if let Some(next) = r.new_start_page_token {
                return Ok((out, next));
            }
            page = r.next_page_token.ok_or_else(|| anyhow!("changes: missing page token"))?;
        }
    }

    pub fn get_file(&self, id: &str) -> Result<DFile> {
        self.get_json(
            &format!("{API}/files/{id}"),
            &[("fields", FILE_FIELDS), ("supportsAllDrives", "true")],
        )
    }

    fn patch(&self, id: &str, body: serde_json::Value, extra: &[(&str, &str)]) -> Result<DFile> {
        let url = format!("{API}/files/{id}");
        let mut q = vec![("fields", FILE_FIELDS), ("supportsAllDrives", "true")];
        q.extend_from_slice(extra);
        Ok(self.exec(|c| c.patch(&url).query(&q).json(&body))?.json()?)
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<DFile> {
        self.patch(id, json!({ "name": name }), &[])
    }
    pub fn set_starred(&self, id: &str, starred: bool) -> Result<DFile> {
        self.patch(id, json!({ "starred": starred }), &[])
    }
    pub fn set_trashed(&self, id: &str, trashed: bool) -> Result<DFile> {
        self.patch(id, json!({ "trashed": trashed }), &[])
    }
    pub fn move_to(&self, id: &str, new_parent: &str, old_parents: &[String]) -> Result<DFile> {
        let remove = old_parents.join(",");
        self.patch(id, json!({}), &[("addParents", new_parent), ("removeParents", &remove)])
    }

    pub fn create_folder(&self, name: &str, parent: &str) -> Result<DFile> {
        let url = format!("{API}/files");
        let body = json!({ "name": name, "mimeType": FOLDER_MIME, "parents": [parent] });
        let q = [("fields", FILE_FIELDS), ("supportsAllDrives", "true")];
        Ok(self.exec(|c| c.post(&url).query(&q).json(&body))?.json()?)
    }

    /// Make the file readable by anyone with the link.
    pub fn share_anyone(&self, id: &str) -> Result<()> {
        let url = format!("{API}/files/{id}/permissions");
        let body = json!({ "role": "reader", "type": "anyone" });
        let q = [("supportsAllDrives", "true"), ("fields", "id")];
        self.exec(|c| c.post(&url).query(&q).json(&body))?;
        Ok(())
    }

    /// Full-text (content) search, server side.
    pub fn search_content(&self, text: &str) -> Result<Vec<DFile>> {
        let escaped = text.replace('\\', "\\\\").replace('\'', "\\'");
        let q = format!("fullText contains '{escaped}' and trashed = false");
        let fields = format!("files({FILE_FIELDS})");
        #[derive(Deserialize)]
        struct R {
            #[serde(default)]
            files: Vec<DFile>,
        }
        let r: R = self.get_json(
            &format!("{API}/files"),
            &[
                ("q", q.as_str()),
                ("pageSize", "200"),
                ("fields", &fields),
                ("corpora", "allDrives"),
                ("supportsAllDrives", "true"),
                ("includeItemsFromAllDrives", "true"),
            ],
        )?;
        Ok(r.files)
    }

    /// Upload a local file into `parent`. `progress(sent, total)` is called as bytes go out.
    pub fn upload(&self, path: &Path, parent: &str, mut progress: impl FnMut(u64, u64)) -> Result<DFile> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow!("bad file name"))?
            .to_string();
        let len = std::fs::metadata(path)?.len();
        let meta = json!({ "name": name, "parents": [parent] });
        let q = [("fields", FILE_FIELDS), ("supportsAllDrives", "true")];

        if len <= SIMPLE_UPLOAD_MAX {
            let data = std::fs::read(path)?;
            let boundary = format!("dray{}", std::process::id());
            let mut body = Vec::with_capacity(data.len() + 512);
            write!(
                body,
                "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n"
            )?;
            body.extend_from_slice(&data);
            write!(body, "\r\n--{boundary}--\r\n")?;
            let url = format!("{UPLOAD_API}/files?uploadType=multipart");
            let ct = format!("multipart/related; boundary={boundary}");
            let resp = self.exec(|c| {
                c.post(&url)
                    .query(&q)
                    .header("Content-Type", &ct)
                    .timeout(Duration::from_secs(600))
                    .body(body.clone())
            })?;
            progress(len, len);
            return Ok(resp.json()?);
        }

        // Resumable upload for large files.
        let url = format!("{UPLOAD_API}/files?uploadType=resumable");
        let resp = self.exec(|c| {
            c.post(&url)
                .query(&q)
                .header("X-Upload-Content-Length", len.to_string())
                .json(&meta)
        })?;
        let session = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| anyhow!("no upload session URL"))?
            .to_string();
        let mut file = std::fs::File::open(path)?;
        let mut sent: u64 = 0;
        let mut buf = vec![0u8; CHUNK];
        loop {
            let mut n = 0;
            while n < CHUNK {
                let r = file.read(&mut buf[n..])?;
                if r == 0 {
                    break;
                }
                n += r;
            }
            let end = sent + n as u64;
            let range = if n == 0 {
                format!("bytes */{len}")
            } else {
                format!("bytes {}-{}/{}", sent, end - 1, len)
            };
            let chunk = buf[..n].to_vec();
            let resp = self.exec_with(&self.upload_http, |c| {
                c.put(&session)
                    .header("Content-Range", &range)
                    .timeout(Duration::from_secs(900))
                    .body(chunk.clone())
            })?;
            sent = end;
            progress(sent, len);
            if resp.status() != StatusCode::PERMANENT_REDIRECT {
                return Ok(resp.json()?);
            }
            if n == 0 {
                bail!("upload ended without a final response");
            }
        }
    }

    /// Download (or export, for Google Docs/Sheets/Slides) into `dir`, or over `overwrite` if
    /// given (refreshing an earlier download). Returns the written path.
    pub fn download(
        &self,
        f: &DFile,
        dir: &Path,
        overwrite: Option<&Path>,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<PathBuf> {
        let (url, query, ext): (String, Vec<(&str, &str)>, Option<&str>) = if f.is_google_native() {
            let (mime, ext) = match f.mime_type.as_str() {
                "application/vnd.google-apps.document" => (
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                    "docx",
                ),
                "application/vnd.google-apps.spreadsheet" => (
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                    "xlsx",
                ),
                "application/vnd.google-apps.presentation" => (
                    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                    "pptx",
                ),
                "application/vnd.google-apps.drawing" => ("image/png", "png"),
                "application/vnd.google-apps.script" => ("application/vnd.google-apps.script+json", "json"),
                _ => ("application/pdf", "pdf"),
            };
            (format!("{API}/files/{}/export", f.id), vec![("mimeType", mime)], Some(ext))
        } else {
            (
                format!("{API}/files/{}", f.id),
                vec![("alt", "media"), ("supportsAllDrives", "true")],
                None,
            )
        };
        let mut name = sanitize(&f.name);
        if let Some(ext) = ext {
            if !name.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
                name = format!("{name}.{ext}");
            }
        }
        let dest = match overwrite {
            Some(p) => p.to_path_buf(),
            None => {
                std::fs::create_dir_all(dir)?;
                unique_path(dir, &name)
            }
        };
        let mut resp = self.exec(|c| c.get(&url).query(&query).timeout(Duration::from_secs(3600)))?;
        let total = resp.content_length().or(f.size_bytes());
        let mut part = dest.clone().into_os_string();
        part.push(".part-gdo");
        let part = PathBuf::from(part);
        let mut out = std::fs::File::create(&part).with_context(|| format!("creating {}", part.display()))?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut done = 0u64;
        loop {
            let n = resp.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            done += n as u64;
            progress(done, total);
        }
        out.sync_all()?;
        drop(out);
        std::fs::rename(&part, &dest)?;
        Ok(dest)
    }
}

fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let s = s.trim().trim_start_matches('.').to_string();
    if s.is_empty() { "untitled".into() } else { s }
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for i in 1.. {
        let p = dir.join(format!("{stem} ({i}){ext}"));
        if !p.exists() {
            return p;
        }
    }
    unreachable!()
}
