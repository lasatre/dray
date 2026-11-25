//! Interface icons: Lucide (ISC license, see assets/icons/LICENSE). The SVGs are stored
//! white with a 2.25-unit stroke, which is 1.5 px at 16 px, so egui can tint them: `ink-muted`
//! at rest, `ink` when selected, `brand` for the folder icon.

use crate::model::Kind;
use eframe::egui::{self, Color32, ImageSource, include_image};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    BookOpen,
    Clock,
    Download,
    ExternalLink,
    File,
    FileArchive,
    FileAudio,
    FileCheck,
    FileCode,
    FileImage,
    FileSpreadsheet,
    FileSymlink,
    FileText,
    FileType,
    FileVideo,
    Folder,
    FolderOpen,
    FolderPlus,
    Globe,
    HardDrive,
    HardDriveDownload,
    Hash,
    Info,
    Link,
    MapPin,
    PenTool,
    Pencil,
    Pin,
    Presentation,
    RefreshCw,
    RotateCcw,
    Scissors,
    Search,
    Server,
    Settings,
    Share2,
    Star,
    StarOff,
    Trash2,
    Type,
    Upload,
    Users,
    X,
}

impl Icon {
    pub fn source(self) -> ImageSource<'static> {
        match self {
            Icon::ArrowLeft => include_image!("../assets/icons/arrow-left.svg"),
            Icon::ArrowRight => include_image!("../assets/icons/arrow-right.svg"),
            Icon::ArrowUp => include_image!("../assets/icons/arrow-up.svg"),
            Icon::BookOpen => include_image!("../assets/icons/book-open.svg"),
            Icon::Clock => include_image!("../assets/icons/clock.svg"),
            Icon::Download => include_image!("../assets/icons/download.svg"),
            Icon::ExternalLink => include_image!("../assets/icons/external-link.svg"),
            Icon::File => include_image!("../assets/icons/file.svg"),
            Icon::FileArchive => include_image!("../assets/icons/file-archive.svg"),
            Icon::FileAudio => include_image!("../assets/icons/file-audio.svg"),
            Icon::FileCheck => include_image!("../assets/icons/file-check.svg"),
            Icon::FileCode => include_image!("../assets/icons/file-code.svg"),
            Icon::FileImage => include_image!("../assets/icons/file-image.svg"),
            Icon::FileSpreadsheet => include_image!("../assets/icons/file-spreadsheet.svg"),
            Icon::FileSymlink => include_image!("../assets/icons/file-symlink.svg"),
            Icon::FileText => include_image!("../assets/icons/file-text.svg"),
            Icon::FileType => include_image!("../assets/icons/file-type.svg"),
            Icon::FileVideo => include_image!("../assets/icons/file-video.svg"),
            Icon::Folder => include_image!("../assets/icons/folder.svg"),
            Icon::FolderOpen => include_image!("../assets/icons/folder-open.svg"),
            Icon::FolderPlus => include_image!("../assets/icons/folder-plus.svg"),
            Icon::Globe => include_image!("../assets/icons/globe.svg"),
            Icon::HardDrive => include_image!("../assets/icons/hard-drive.svg"),
            Icon::HardDriveDownload => include_image!("../assets/icons/hard-drive-download.svg"),
            Icon::Hash => include_image!("../assets/icons/hash.svg"),
            Icon::Info => include_image!("../assets/icons/info.svg"),
            Icon::Link => include_image!("../assets/icons/link.svg"),
            Icon::MapPin => include_image!("../assets/icons/map-pin.svg"),
            Icon::PenTool => include_image!("../assets/icons/pen-tool.svg"),
            Icon::Pencil => include_image!("../assets/icons/pencil.svg"),
            Icon::Pin => include_image!("../assets/icons/pin.svg"),
            Icon::Presentation => include_image!("../assets/icons/presentation.svg"),
            Icon::RefreshCw => include_image!("../assets/icons/refresh-cw.svg"),
            Icon::RotateCcw => include_image!("../assets/icons/rotate-ccw.svg"),
            Icon::Scissors => include_image!("../assets/icons/scissors.svg"),
            Icon::Search => include_image!("../assets/icons/search.svg"),
            Icon::Server => include_image!("../assets/icons/server.svg"),
            Icon::Settings => include_image!("../assets/icons/settings.svg"),
            Icon::Share2 => include_image!("../assets/icons/share-2.svg"),
            Icon::Star => include_image!("../assets/icons/star.svg"),
            Icon::StarOff => include_image!("../assets/icons/star-off.svg"),
            Icon::Trash2 => include_image!("../assets/icons/trash-2.svg"),
            Icon::Type => include_image!("../assets/icons/type.svg"),
            Icon::Upload => include_image!("../assets/icons/upload.svg"),
            Icon::Users => include_image!("../assets/icons/users.svg"),
            Icon::X => include_image!("../assets/icons/x.svg"),
        }
    }

    /// The icon at `size` points (16 in lists and menus, 20 in the toolbar), tinted.
    pub fn image(self, size: f32, tint: Color32) -> egui::Image<'static> {
        egui::Image::new(self.source()).fit_to_exact_size(egui::vec2(size, size)).tint(tint)
    }

    /// File types are told apart by shape (and the name), never by color.
    pub fn for_kind(kind: Kind) -> Icon {
        match kind {
            Kind::Folder => Icon::Folder,
            Kind::Shortcut => Icon::FileSymlink,
            Kind::Doc => Icon::FileText,
            Kind::Sheet => Icon::FileSpreadsheet,
            Kind::Slides => Icon::Presentation,
            Kind::Form => Icon::FileCheck,
            Kind::Drawing => Icon::PenTool,
            Kind::Pdf => Icon::BookOpen,
            Kind::Image => Icon::FileImage,
            Kind::Video => Icon::FileVideo,
            Kind::Audio => Icon::FileAudio,
            Kind::Archive => Icon::FileArchive,
            Kind::Code => Icon::FileCode,
            Kind::Text => Icon::FileType,
            Kind::Other => Icon::File,
        }
    }
}
