#!/usr/bin/env bash
# Build a Linux AppImage into dist/. Run on an old-ish glibc distro (e.g. Debian bookworm)
# so the result runs on most current distributions.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"

VERSION="${VERSION:-$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)}"
ARCH="$(uname -m)"
cargo build --release --locked

APPDIR="target/AppDir"
rm -rf "$APPDIR"
install -Dm755 target/release/dray "$APPDIR/usr/bin/dray"
install -Dm644 packaging/dray.desktop "$APPDIR/usr/share/applications/dray.desktop"
install -Dm644 packaging/dray.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/dray.svg"
cp packaging/dray.desktop packaging/dray.svg "$APPDIR/"
ln -sf dray.svg "$APPDIR/.DirIcon"
cat > "$APPDIR/AppRun" <<'RUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/dray" "$@"
RUN
chmod +x "$APPDIR/AppRun"

# The binary only links against glibc; X11/Wayland/GL libraries are loaded from the host
# at runtime (they must not be bundled), so no linuxdeploy step is needed.
TOOL="target/appimagetool-$ARCH.AppImage"
if [ ! -x "$TOOL" ]; then
  curl -fsSL -o "$TOOL" \
    "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
  chmod +x "$TOOL"
fi
mkdir -p dist
# Extract-and-run: CI containers have no FUSE.
# Absolute paths: in extract-and-run mode appimagetool mis-resolves relative ones.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$ARCH" "$ROOT/$TOOL" --no-appstream "$ROOT/$APPDIR" \
  "$ROOT/dist/Dray-$VERSION-$ARCH.AppImage"
ls -la dist
