use serde::{Deserialize, Serialize};

pub const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
pub const SHORTCUT_MIME: &str = "application/vnd.google-apps.shortcut";

/// Fields requested for every file, everywhere. Keep in sync with [`DFile`].
pub const FILE_FIELDS: &str = "id,name,mimeType,parents,size,modifiedTime,viewedByMeTime,\
webViewLink,starred,trashed,ownedByMe,shared,driveId,shortcutDetails(targetId,targetMimeType)";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutDetails {
    #[serde(default)]
    pub target_id: String,
    #[serde(default)]
    pub target_mime_type: String,
}

/// A Drive file as returned by the API (and as stored in the cache).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DFile {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewed_by_me_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_view_link: Option<String>,
    #[serde(default)]
    pub starred: bool,
    #[serde(default)]
    pub trashed: bool,
    #[serde(default)]
    pub owned_by_me: bool,
    #[serde(default)]
    pub shared: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut_details: Option<ShortcutDetails>,
}

impl DFile {
    pub fn is_folder(&self) -> bool {
        self.mime_type == FOLDER_MIME
    }

    pub fn is_google_native(&self) -> bool {
        self.mime_type.starts_with("application/vnd.google-apps.")
    }

    /// If this is a shortcut to a folder, the target folder id.
    pub fn folder_target(&self) -> Option<&str> {
        if self.is_folder() {
            return Some(&self.id);
        }
        match &self.shortcut_details {
            Some(s) if self.mime_type == SHORTCUT_MIME && s.target_mime_type == FOLDER_MIME => {
                Some(&s.target_id)
            }
            _ => None,
        }
    }

    pub fn size_bytes(&self) -> Option<u64> {
        self.size.as_deref().and_then(|s| s.parse().ok())
    }

    pub fn view_link(&self) -> String {
        self.web_view_link
            .clone()
            .unwrap_or_else(|| format!("https://drive.google.com/file/d/{}/view", self.id))
    }

    /// A link that downloads the file directly (only meaningful for binary files).
    pub fn download_link(&self) -> String {
        if self.is_google_native() {
            self.view_link()
        } else {
            format!("https://drive.google.com/uc?export=download&id={}", self.id)
        }
    }

    /// Best-effort "last touched" timestamp (RFC3339 strings sort lexicographically).
    pub fn recency(&self) -> &str {
        let m = self.modified_time.as_deref().unwrap_or("");
        let v = self.viewed_by_me_time.as_deref().unwrap_or("");
        if v > m { v } else { m }
    }

    pub fn extension(&self) -> Option<&str> {
        let (_, ext) = self.name.rsplit_once('.')?;
        if ext.is_empty() || ext.len() > 8 { None } else { Some(ext) }
    }

    pub fn kind(&self) -> Kind {
        Kind::of(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Folder,
    Shortcut,
    Doc,
    Sheet,
    Slides,
    Form,
    Drawing,
    Pdf,
    Image,
    Video,
    Audio,
    Archive,
    Code,
    Text,
    Other,
}

impl Kind {
    pub fn of(f: &DFile) -> Kind {
        let m = f.mime_type.as_str();
        match m {
            FOLDER_MIME => return Kind::Folder,
            SHORTCUT_MIME => return Kind::Shortcut,
            "application/vnd.google-apps.document" => return Kind::Doc,
            "application/vnd.google-apps.spreadsheet" => return Kind::Sheet,
            "application/vnd.google-apps.presentation" => return Kind::Slides,
            "application/vnd.google-apps.form" => return Kind::Form,
            "application/vnd.google-apps.drawing" => return Kind::Drawing,
            "application/pdf" => return Kind::Pdf,
            _ => {}
        }
        if m.starts_with("image/") {
            return Kind::Image;
        }
        if m.starts_with("video/") {
            return Kind::Video;
        }
        if m.starts_with("audio/") {
            return Kind::Audio;
        }
        let ext = f.extension().map(|e| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("pdf") => Kind::Pdf,
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "heic" | "svg" | "psd" | "raw") => Kind::Image,
            Some("mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v" | "wmv") => Kind::Video,
            Some("mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus") => Kind::Audio,
            Some("zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "xz" | "bz2" | "zst") => Kind::Archive,
            Some("doc" | "docx" | "odt" | "rtf") => Kind::Doc,
            Some("xls" | "xlsx" | "ods" | "csv" | "tsv") => Kind::Sheet,
            Some("ppt" | "pptx" | "odp" | "key") => Kind::Slides,
            Some(
                "rs" | "c" | "h" | "cpp" | "hpp" | "cs" | "py" | "js" | "ts" | "go" | "java" | "kt"
                | "rb" | "sh" | "json" | "toml" | "yaml" | "yml" | "html" | "css" | "xml" | "sql",
            ) => Kind::Code,
            Some("txt" | "md" | "log") => Kind::Text,
            _ if m.starts_with("text/") => Kind::Text,
            _ => Kind::Other,
        }
    }

    /// Parse a `type:` search filter value.
    pub fn parse_filter(s: &str) -> Option<Kind> {
        Some(match s {
            "folder" | "dir" => Kind::Folder,
            "shortcut" => Kind::Shortcut,
            "doc" | "docs" | "document" => Kind::Doc,
            "sheet" | "sheets" | "spreadsheet" | "xls" => Kind::Sheet,
            "slide" | "slides" | "ppt" | "presentation" => Kind::Slides,
            "form" => Kind::Form,
            "drawing" => Kind::Drawing,
            "pdf" => Kind::Pdf,
            "image" | "img" | "photo" | "pic" => Kind::Image,
            "video" | "vid" | "movie" => Kind::Video,
            "audio" | "music" | "sound" => Kind::Audio,
            "zip" | "archive" => Kind::Archive,
            "code" => Kind::Code,
            "text" | "txt" => Kind::Text,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SharedDrive {
    pub id: String,
    pub name: String,
}
