<img src="brand/logo/app-icon.svg" width="72" height="72" alt="">

# Dray

A fast native desktop front-end for Google Drive, written in Rust (egui).

It keeps your whole Drive's metadata in a local cache, so search is instant and works
offline. Changes come in through the Drive Changes API, and anything you upload, rename or
move goes into the cache the moment Google responds. Links are copied from the cache, so
copying one needs no network request.

## Features

- **Instant search** over every file name in My Drive, Shared with me and shared drives.
  Search is multi-word and ignores punctuation (`tax 2024` matches `Tax_Return-2024.pdf`).
  Words can also match the folder path. Results are ranked (prefix > word start > substring),
  with recent files first on ties. 200k files search in roughly 2–10 ms.
  - Filters: `type:pdf|doc|sheet|slide|folder|image|video|audio|zip|code`, `ext:psd`,
    `is:starred`, `is:shared`, `is:mine`, `is:trashed`, `"exact phrase"`
  - **📁 This folder** (Ctrl+Shift+F) limits the search to the open folder and its
    subfolders. It stays on as you move between folders.
  - **Shift+Enter** searches inside file contents (Drive full-text search, done server-side).
- **Launcher-style keyboard flow**: type, use ↑/↓, then **Enter** to open or **Ctrl+C** to
  copy the link. **Ctrl+Shift+S** shares the file as "anyone with link" and copies the link.
- **Drag & drop uploads, like the web UI**: drop files or whole folders onto a folder row, a
  sidebar entry or a breadcrumb to upload into that folder. Dropping anywhere else uploads into
  the folder you're viewing. Ctrl+U opens a file picker instead. Up to 4 uploads run in
  parallel. Large files use resumable uploads. When the upload finishes, the file is
  selected right away and (optionally) its link is copied.
- **Downloads are remembered**: after you download a file, double-clicking it (or pressing
  Enter) opens your local copy instead of the web page. A 💾 marks downloaded files and turns
  orange when Drive has a newer version; Ctrl+S then updates the copy in place. Downloads go to
  your Downloads folder by default; change it in Settings.
- **Startup folder**: open the app in any folder (right-click → "Open this folder at startup",
  or Settings → Open at startup). You can also start in Recent, Starred or Shared.
- Browse My Drive, Recent, Starred, Shared with me, Trash and shared drives. You can
  rename, star, trash/restore, move (Ctrl+X → Ctrl+V), create folders and download. Google
  Docs/Sheets/Slides are exported as .docx/.xlsx/.pptx.
- The first index is built with up to 8 parallel listing requests and streams into the UI.
  After that, the cache is ready at startup and only changes are fetched (every 15 s by
  default, or press F5).

## Build

```sh
# needs a Rust toolchain (https://rustup.rs) and a C compiler (gcc/clang) for bundled SQLite
cargo build --release
./target/release/dray
```

### Packages (CI)

`.forgejo/workflows/build.yml` runs on every push and PR. It builds:

- `Dray-<version>-x86_64.AppImage`, built on Debian bookworm, so it runs on any
  distro with glibc ≥ 2.36
- `Dray-<version>-windows-x64.zip` (a standalone `Dray.exe` + README),
  cross-compiled with mingw-w64; it has no DLL dependencies beyond Windows' own

Both files are attached to the workflow run as artifacts. When you push a `v*` tag
(e.g. `git tag v0.1.0 && git push --tags`), they're also published to a Forgejo release.
The workflow needs a runner with the `docker` label. The same steps work locally:

```sh
packaging/build-appimage.sh   # -> dist/*.AppImage
packaging/build-windows.sh    # -> dist/*.zip  (needs mingw-w64 + the x86_64-pc-windows-gnu target)
```

## One-time Google setup

Google requires each desktop app to have its own OAuth client. It's free and takes about
5 minutes:

1. Go to <https://console.cloud.google.com/>, create a project, and enable the
   **Google Drive API** (APIs & Services → Library).
2. Go to **OAuth consent screen** (Google Auth Platform): choose External, fill in the app
   name and your email, and add yourself under **Test users**.
3. Go to **Credentials → Create credentials → OAuth client ID → Application type: Desktop
   app**, and download the JSON.
4. Start Dray. Drop the JSON on the window (or paste the client ID and secret),
   then click **Sign in with Google**.

> **Tip:** while the consent screen is in *Testing* status, Google expires refresh tokens
> after 7 days, so you'd have to sign in again every week. To avoid that, click
> **Publish app** on the consent screen. You'll see an "unverified app" warning once at
> sign-in, which is expected for a personal app.

## Keyboard shortcuts

| Keys | Action |
|---|---|
| just type / Ctrl+F / Ctrl+K | Search |
| Ctrl+Shift+F | Search only the open folder (toggle) |
| ↑ ↓ PgUp PgDn (Shift to extend) | Move selection, even while typing |
| Enter / double-click | Open the local copy if downloaded, else in the browser; enter folders |
| Ctrl+Enter | Show the result in its folder |
| Shift+Enter | Search file contents on Drive |
| Ctrl+C / Ctrl+Shift+C | Copy link(s) |
| Ctrl+Shift+D | Copy direct-download link |
| Ctrl+Shift+S | Share "anyone with link" and copy the link |
| Ctrl+O | Open in browser |
| Ctrl+S | Download (or update the local copy) |
| Ctrl+U / drag & drop | Upload (drop onto a folder to upload into it) |
| Ctrl+Shift+N | New folder |
| F2 | Rename |
| Delete | Trash (Restore in Trash view) |
| Ctrl+X, then Ctrl+V | Move to another folder |
| Backspace / Alt+↑ | Parent folder |
| Alt+← / Alt+→ | Back / forward |
| Esc | Clear search |
| F5 / Ctrl+R | Sync now |

## Linux: X11 vs Wayland

winit, the windowing library, doesn't support file drag-and-drop on native Wayland yet. So on
Linux the app runs through XWayland whenever `DISPLAY` is set (KDE and GNOME scale XWayland
apps crisply by default). `DRAY_WAYLAND=1` forces native Wayland, but drag-and-drop upload
won't work there.

`vendor/winit` is winit 0.30.13 with one small patch that reports the cursor position during an
XDND drag, so drops can target a specific folder. See `vendor/winit/PATCHED.md`.

## Where things are stored

- `~/.config/dray/`: `client.json` (OAuth client) and `token.json` (refresh
  token, mode 0600), plus `settings.json`
- downloaded-file locations live in the cache database's `local_copies` table
- `~/.local/share/dray/cache.sqlite3`: metadata cache (Settings → Rebuild cache
  wipes and re-lists it)

## Demo mode

`DRAY_DEMO=200000 ./target/release/dray` starts without signing in, with 200k
synthetic files. It's useful for trying out the UI and search speed.

## Layout

| File | Purpose |
|---|---|
| `src/auth.rs` | OAuth loopback + PKCE sign-in, token refresh |
| `src/drive.rs` | Drive v3 REST client (list, changes, upload, download, share, …) with retry/backoff |
| `src/cache.rs` | SQLite metadata cache |
| `src/backend.rs` | Background sync (parallel full listing + Changes API) and file operations |
| `src/index.rs` | In-memory index: folder paths, children, ranked parallel search |
| `src/app.rs` | egui UI |
