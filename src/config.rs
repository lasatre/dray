use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn new() -> Result<Paths> {
        let dirs = directories::ProjectDirs::from("", "", "gdriveoperator")
            .ok_or_else(|| anyhow!("could not determine home directory"))?;
        let p = Paths {
            config_dir: dirs.config_dir().to_path_buf(),
            data_dir: dirs.data_local_dir().to_path_buf(),
        };
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
}

impl Default for Settings {
    fn default() -> Self {
        Settings { copy_link_after_upload: true, poll_seconds: 15, dark_mode: None }
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
