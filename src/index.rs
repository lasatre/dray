//! In-memory index over the cached metadata: folder paths, children lists and instant search.

use crate::model::{DFile, Kind};
use std::collections::HashMap;
use std::sync::Arc;

pub struct Entry {
    pub file: Arc<DFile>,
    pub kind: Kind,
    pub name_norm: String,
    /// Display path of the containing folder ("My Drive/Projects").
    pub path: Arc<str>,
    /// Index into [`Index::path_norms`].
    pub path_id: u32,
}

#[derive(Default)]
pub struct Index {
    pub entries: Vec<Entry>,
    pub by_id: HashMap<String, usize>,
    pub children: HashMap<String, Vec<usize>>,
    /// Normalised folder paths, shared by all entries in the same folder.
    pub path_norms: Vec<Arc<str>>,
}

type PathMemo = HashMap<String, (Arc<str>, u32)>;

/// Lowercase, and turn punctuation (`_`, `-`, `.`, ...) into spaces so "my_report-v2.pdf"
/// matches the query "report v2".
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_alphanumeric() {
            for l in c.to_lowercase() {
                out.push(l);
            }
        } else {
            out.push(' ');
        }
    }
    out
}

pub struct Roots<'a> {
    pub my_drive: Option<&'a str>,
    pub drives: &'a HashMap<String, String>,
}

impl Index {
    pub fn build(files: &HashMap<String, Arc<DFile>>, roots: &Roots) -> Index {
        let mut memo: PathMemo = HashMap::new();
        let mut path_norms: Vec<Arc<str>> = Vec::new();
        let mut entries = Vec::with_capacity(files.len());
        for f in files.values() {
            let (path, path_id) = match f.parents.first() {
                Some(p) => folder_path(p, files, roots, &mut memo, &mut path_norms, 0),
                None => {
                    let key = if f.owned_by_me { "\0orphan" } else { "\0shared" };
                    let label = if f.owned_by_me { "" } else { "Shared with me" };
                    intern(key, label.to_string(), &mut memo, &mut path_norms)
                }
            };
            entries.push(Entry { kind: f.kind(), name_norm: normalize(&f.name), path, path_id, file: f.clone() });
        }
        let mut by_id = HashMap::with_capacity(entries.len());
        let mut children: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            by_id.insert(e.file.id.clone(), i);
            for p in &e.file.parents {
                children.entry(p.clone()).or_default().push(i);
            }
        }
        Index { entries, by_id, children, path_norms }
    }

    #[allow(dead_code)]
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.by_id.get(id).map(|&i| &self.entries[i])
    }

    pub fn search(&self, q: &Query, limit: usize) -> (Vec<usize>, usize) {
        // Which tokens occur in each folder path: computed once per folder, not per file.
        let tokens = &q.tokens[..q.tokens.len().min(64)];
        let path_hits: Vec<u64> = self
            .path_norms
            .iter()
            .map(|p| {
                tokens.iter().enumerate().fold(0u64, |m, (i, t)| if p.contains(t.as_str()) { m | 1 << i } else { m })
            })
            .collect();
        let scan = |range: std::ops::Range<usize>| -> Vec<(i32, usize)> {
            let mut out = Vec::new();
            for i in range {
                let e = &self.entries[i];
                if !q.accepts(e) {
                    continue;
                }
                if let Some(s) = score(e, tokens, &q.whole, path_hits[e.path_id as usize]) {
                    out.push((s, i));
                }
            }
            out
        };
        let n = self.entries.len();
        let threads = std::thread::available_parallelism().map(|t| t.get()).unwrap_or(1).min(8);
        let mut scored: Vec<(i32, usize)> = if n < 20_000 || threads < 2 {
            scan(0..n)
        } else {
            let chunk = n.div_ceil(threads);
            std::thread::scope(|sc| {
                let handles: Vec<_> = (0..threads)
                    .map(|t| {
                        let scan = &scan;
                        sc.spawn(move || scan(t * chunk..((t + 1) * chunk).min(n)))
                    })
                    .collect();
                handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
            })
        };
        let total = scored.len();
        let cmp = |a: &(i32, usize), b: &(i32, usize)| {
            b.0.cmp(&a.0).then_with(|| {
                self.entries[b.1].file.recency().cmp(self.entries[a.1].file.recency())
            })
        };
        if scored.len() > limit {
            scored.select_nth_unstable_by(limit, cmp);
            scored.truncate(limit);
        }
        scored.sort_unstable_by(cmp);
        (scored.into_iter().map(|(_, i)| i).collect(), total)
    }
}

fn intern(key: &str, display: String, memo: &mut PathMemo, norms: &mut Vec<Arc<str>>) -> (Arc<str>, u32) {
    let id = norms.len() as u32;
    norms.push(Arc::from(normalize(&display).as_str()));
    let v = (Arc::<str>::from(display.as_str()), id);
    memo.insert(key.to_string(), v.clone());
    v
}

fn folder_path(
    id: &str,
    files: &HashMap<String, Arc<DFile>>,
    roots: &Roots,
    memo: &mut PathMemo,
    norms: &mut Vec<Arc<str>>,
    depth: usize,
) -> (Arc<str>, u32) {
    if let Some(p) = memo.get(id) {
        return p.clone();
    }
    let display: String = if Some(id) == roots.my_drive {
        "My Drive".into()
    } else if let Some(name) = roots.drives.get(id) {
        name.clone()
    } else if let Some(f) = files.get(id) {
        match f.parents.first() {
            Some(parent) if depth < 64 => {
                let (pp, _) = folder_path(parent, files, roots, memo, norms, depth + 1);
                format!("{pp}/{}", f.name)
            }
            _ if f.owned_by_me => f.name.clone(),
            _ => format!("Shared with me/{}", f.name),
        }
    } else if roots.my_drive.is_none() {
        // Root id not known yet (first sync in progress).
        "My Drive".into()
    } else {
        "Shared with me".into()
    };
    intern(id, display, memo, norms)
}

fn score(e: &Entry, tokens: &[String], whole: &str, path_hits: u64) -> Option<i32> {
    if tokens.is_empty() {
        return Some(0);
    }
    let name = e.name_norm.as_str();
    let mut s = 0i32;
    for (i, t) in tokens.iter().enumerate() {
        if let Some(pos) = name.find(t.as_str()) {
            if pos == 0 {
                s += 100;
            } else if word_start(name, t) {
                s += 70;
            } else {
                s += 35;
            }
        } else if path_hits & (1 << i) != 0 {
            s += 8;
        } else {
            return None;
        }
    }
    let trimmed = name.trim();
    if trimmed == whole {
        s += 400;
    } else if trimmed.starts_with(whole) {
        s += 120;
    } else if name.contains(whole) {
        s += 50;
    }
    if e.kind == Kind::Folder {
        s += 5;
    }
    // Prefer shorter names when the match is otherwise equal.
    s -= (name.len() / 16).min(10) as i32;
    Some(s)
}

fn word_start(hay: &str, needle: &str) -> bool {
    hay.match_indices(needle)
        .any(|(i, _)| i == 0 || hay.as_bytes()[i - 1] == b' ')
}

#[derive(Default, Debug)]
pub struct Query {
    pub tokens: Vec<String>,
    /// All free-text tokens joined by single spaces (for exact/prefix bonuses).
    pub whole: String,
    pub kind: Option<Kind>,
    pub ext: Option<String>,
    pub starred: bool,
    pub trashed: bool,
    pub shared: bool,
    pub mine: bool,
}

impl Query {
    /// Parses e.g. `budget 2024 type:sheet is:starred "exact phrase"`.
    fn accepts(&self, e: &Entry) -> bool {
        let f = &*e.file;
        if f.trashed != self.trashed
            || (self.starred && !f.starred)
            || (self.shared && !(f.shared || !f.owned_by_me))
            || (self.mine && !f.owned_by_me)
        {
            return false;
        }
        if let Some(k) = self.kind {
            if e.kind != k && !(k == Kind::Folder && f.folder_target().is_some()) {
                return false;
            }
        }
        if let Some(ext) = &self.ext {
            return f.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext));
        }
        true
    }

    pub fn parse(raw: &str) -> Query {
        let mut q = Query::default();
        let mut words: Vec<String> = Vec::new();
        let mut rest = raw.trim();
        while !rest.is_empty() {
            if let Some(stripped) = rest.strip_prefix('"') {
                let end = stripped.find('"').unwrap_or(stripped.len());
                let phrase = normalize(&stripped[..end]);
                let phrase = phrase.split_whitespace().collect::<Vec<_>>().join(" ");
                if !phrase.is_empty() {
                    words.push(phrase);
                }
                rest = stripped.get(end + 1..).unwrap_or("").trim_start();
                continue;
            }
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let word = &rest[..end];
            rest = rest[end..].trim_start();
            let lower = word.to_lowercase();
            if let Some(v) = lower.strip_prefix("type:") {
                if let Some(k) = Kind::parse_filter(v) {
                    q.kind = Some(k);
                    continue;
                }
            } else if let Some(v) = lower.strip_prefix("ext:") {
                q.ext = Some(v.trim_start_matches('.').to_string());
                continue;
            } else if let Some(v) = lower.strip_prefix("is:") {
                match v {
                    "starred" => q.starred = true,
                    "trashed" | "trash" => q.trashed = true,
                    "shared" => q.shared = true,
                    "mine" => q.mine = true,
                    _ => {}
                }
                continue;
            }
            words.extend(normalize(word).split_whitespace().map(str::to_string));
        }
        q.whole = words.join(" ");
        q.tokens = words;
        q
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: &str, name: &str, parent: &str) -> Arc<DFile> {
        Arc::new(DFile {
            id: id.into(),
            name: name.into(),
            mime_type: if name.contains('.') { "application/pdf".into() } else { crate::model::FOLDER_MIME.into() },
            parents: vec![parent.into()],
            size: None,
            modified_time: Some("2024-01-01T00:00:00.000Z".into()),
            viewed_by_me_time: None,
            web_view_link: None,
            starred: false,
            trashed: false,
            owned_by_me: true,
            shared: false,
            drive_id: None,
            shortcut_details: None,
        })
    }

    #[test]
    fn search_ranks_and_paths() {
        let mut files = HashMap::new();
        for f in [
            file("a", "Projects", "root"),
            file("b", "tax_report-2024.pdf", "a"),
            file("c", "report.pdf", "root"),
            file("d", "unrelated.pdf", "a"),
        ] {
            files.insert(f.id.clone(), f);
        }
        let drives = HashMap::new();
        let idx = Index::build(&files, &Roots { my_drive: Some("root"), drives: &drives });
        assert_eq!(&*idx.get("b").unwrap().path, "My Drive/Projects");

        let (r, n) = idx.search(&Query::parse("report"), 10);
        assert_eq!(n, 2);
        assert_eq!(idx.entries[r[0]].file.id, "c"); // prefix + exact beats mid-word
        let (r, _) = idx.search(&Query::parse("report 2024"), 10);
        assert_eq!(r.len(), 1);
        let (r, _) = idx.search(&Query::parse("projects unrelated"), 10); // path token
        assert_eq!(idx.entries[r[0]].file.id, "d");
        let (r, _) = idx.search(&Query::parse("type:folder"), 10);
        assert_eq!(r.len(), 1);
        let (r, _) = idx.search(&Query::parse("\"tax report\""), 10);
        assert_eq!(r.len(), 1);
    }

    /// `cargo test --release -- --ignored --nocapture bench` for a rough timing.
    #[test]
    #[ignore]
    fn bench_search() {
        let words = ["report", "budget", "photo", "invoice", "draft", "final", "meeting", "notes", "plan", "design"];
        let mut files = HashMap::new();
        for i in 0..200_000u32 {
            let parent = if i < 2000 { "root".to_string() } else { format!("f{}", i % 2000) };
            let name = if i < 2000 {
                format!("Folder {} {}", words[(i % 10) as usize], i)
            } else {
                format!("{}_{}_{}-v{}.pdf", words[(i % 10) as usize], words[(i / 10 % 10) as usize], i, i % 7)
            };
            let f = file(&format!("f{i}"), &name, &parent);
            files.insert(f.id.clone(), f);
        }
        let drives = HashMap::new();
        let t = std::time::Instant::now();
        let idx = Index::build(&files, &Roots { my_drive: Some("root"), drives: &drives });
        println!("build: {:?} for {} entries", t.elapsed(), idx.entries.len());
        for q in ["r", "rep", "report budget", "invoice 1999", "zzz", "type:folder plan"] {
            let t = std::time::Instant::now();
            let (r, n) = idx.search(&Query::parse(q), 5000);
            println!("search {q:?}: {:?} ({n} matches, {} shown)", t.elapsed(), r.len());
        }
    }
}
