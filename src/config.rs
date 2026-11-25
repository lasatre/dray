use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

/// Directory name used by pre-release builds, before the project was named Dray.
const LEGACY_DIR_NAME: &str = "gdriveoperator";

impl Paths {
    /// A throwaway profile for demo mode, so it never reads or writes the real one.
    pub fn demo() -> Result<Paths> {
        let dir = std::env::temp_dir().join("dray-demo");
        std::fs::create_dir_all(&dir)?;
        Ok(Paths { config_dir: dir.clone(), data_dir: dir })
    }

    pub fn new() -> Result<Paths> {
        let dirs = directories::ProjectDirs::from("", "", "dray")
            .ok_or_else(|| anyhow!("could not determine home directory"))?;
        let p = Paths {
            config_dir: dirs.config_dir().to_path_buf(),
            data_dir: dirs.data_local_dir().to_path_buf(),
        };
        if let Some(old) = directories::ProjectDirs::from("", "", LEGACY_DIR_NAME) {
            let mut log = Vec::new();
            for (from, to) in [(old.config_dir(), &p.config_dir), (old.data_local_dir(), &p.data_dir)] {
                match migrate_dir(from, to) {
                    Ok(0) => {}
                    Ok(n) => log.push(format!("copied {n} file(s) from {} to {}", from.display(), to.display())),
                    Err(e) => log.push(format!("could not copy {} to {}: {e:#}", from.display(), to.display())),
                }
            }
            if !log.is_empty() {
                std::fs::create_dir_all(&p.config_dir)?;
                let mut text = log.join("\n");
                text.push_str("\nThe old directories were left in place; delete them once Dray works.\n");
                eprintln!("dray: settings migration\n{text}");
                let _ = std::fs::write(p.config_dir.join("migration.log"), text);
            }
        }
        std::fs::create_dir_all(&p.config_dir)?;
        std::fs::create_dir_all(&p.data_dir)?;
        Ok(p)
    }
    pub fn client_file(&self) -> PathBuf {
        self.config_dir.join("client.json")
    }
    pub fn token_file(&self) -> PathBuf {
        self.config_dir.join("token.json")
    }
    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }
    pub fn cache_file(&self) -> PathBuf {
        self.data_dir.join("cache.sqlite3")
    }
}

/// OAuth client credentials (a "Desktop app" OAuth client from Google Cloud Console).
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ClientCreds {
    pub client_id: String,
    pub client_secret: String,
}

impl ClientCreds {
    /// Accepts either our own format or the `client_secret_*.json` file Google lets you download.
    pub fn parse(text: &str) -> Result<ClientCreds> {
        let v: serde_json::Value = serde_json::from_str(text).context("not valid JSON")?;
        let obj = v
            .get("installed")
            .or_else(|| v.get("web"))
            .unwrap_or(&v);
        let id = obj.get("client_id").and_then(|x| x.as_str());
        let secret = obj.get("client_secret").and_then(|x| x.as_str());
        match (id, secret) {
            (Some(id), Some(secret)) => Ok(ClientCreds {
                client_id: id.trim().to_string(),
                client_secret: secret.trim().to_string(),
            }),
            _ => Err(anyhow!("JSON has no client_id / client_secret")),
        }
    }

    pub fn load(paths: &Paths) -> Option<ClientCreds> {
        let text = std::fs::read_to_string(paths.client_file()).ok()?;
        ClientCreds::parse(&text).ok()
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        write_private(&paths.client_file(), &serde_json::to_vec_pretty(self)?)
    }

    pub fn is_complete(&self) -> bool {
        !self.client_id.trim().is_empty() && !self.client_secret.trim().is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub copy_link_after_upload: bool,
    pub poll_seconds: u64,
    pub dark_mode: Option<bool>,
    /// Where downloads go (default: the OS Downloads folder).
    pub download_dir: Option<std::path::PathBuf>,
    /// What to show at startup: "mydrive", "recent", "starred", "shared" or "folder:<id>".
    pub start_view: Option<String>,
    /// File list density: 28 px rows instead of 36 px.
    pub compact_rows: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { copy_link_after_upload: true, poll_seconds: 15, dark_mode: None, download_dir: None, start_view: None, compact_rows: false }
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> Settings {
        std::fs::read_to_string(paths.settings_file())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, paths: &Paths) {
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(paths.settings_file(), bytes);
        }
    }
}

/// One-time migration from the legacy directory: if `to` doesn't exist yet (or is empty) and
/// `from` does, copy everything from `from` into `to`, keeping file permissions (the token
/// file stays private). `from` is never modified. Returns how many files were copied.
fn migrate_dir(from: &Path, to: &Path) -> Result<usize> {
    if from == to || !from.is_dir() {
        return Ok(0);
    }
    if to.read_dir().is_ok_and(|mut d| d.next().is_some()) {
        return Ok(0);
    }
    fn copy_tree(from: &Path, to: &Path, n: &mut usize) -> Result<()> {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            let dest = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                // On macOS config and data share a directory; don't recurse into ourselves.
                if entry.path() != to {
                    copy_tree(&entry.path(), &dest, n)?;
                }
            } else {
                std::fs::copy(entry.path(), &dest)
                    .with_context(|| format!("copying {}", entry.path().display()))?;
                *n += 1;
            }
        }
        Ok(())
    }
    let mut n = 0;
    copy_tree(from, to, &mut n)?;
    Ok(n)
}

/// Write a file readable only by the current user (tokens / secrets).
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::migrate_dir;
    use std::fs;

    #[test]
    fn migrates_legacy_dir_once_and_keeps_the_old_one() {
        let root = std::env::temp_dir().join(format!("dray-migrate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (old, new) = (root.join("old"), root.join("new"));
        fs::create_dir_all(old.join("sub")).unwrap();
        fs::write(old.join("token.json"), "t").unwrap();
        fs::write(old.join("sub/cache.sqlite3"), "c").unwrap();

        assert_eq!(migrate_dir(&old, &new).unwrap(), 2);
        assert_eq!(fs::read_to_string(new.join("token.json")).unwrap(), "t");
        assert_eq!(fs::read_to_string(new.join("sub/cache.sqlite3")).unwrap(), "c");
        assert!(old.join("token.json").exists(), "old directory must be left alone");

        // Second launch: the new directory already has data, so nothing is copied again.
        fs::write(old.join("token.json"), "changed").unwrap();
        assert_eq!(migrate_dir(&old, &new).unwrap(), 0);
        assert_eq!(fs::read_to_string(new.join("token.json")).unwrap(), "t");

        // Nothing to migrate from.
        assert_eq!(migrate_dir(&root.join("missing"), &root.join("x")).unwrap(), 0);
        let _ = fs::remove_dir_all(&root);
    }
}
