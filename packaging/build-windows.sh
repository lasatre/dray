#!/usr/bin/env bash
# Cross-compile the Windows build from Linux (mingw-w64) and zip it into dist/.
# Needs: rustup target x86_64-pc-windows-gnu, gcc-mingw-w64-x86-64, zip.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="${VERSION:-$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)}"
TARGET=x86_64-pc-windows-gnu
# Statically link the C runtime so the .exe has no mingw DLL dependencies.
export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
cargo build --release --locked --target "$TARGET"

OUT="target/GDriveOperator-$VERSION-windows-x64"
rm -rf "$OUT"
mkdir -p "$OUT" dist
cp "target/$TARGET/release/gdriveoperator.exe" "$OUT/GDriveOperator.exe"
cp README.md "$OUT/"
[ -f LICENSE ] && cp LICENSE "$OUT/"

# Sanity check: list DLL imports; anything beyond system DLLs would need shipping too.
if command -v x86_64-w64-mingw32-objdump >/dev/null; then
  echo "DLL imports:"
  x86_64-w64-mingw32-objdump -p "$OUT/GDriveOperator.exe" | grep 'DLL Name' | sort -u
fi

rm -f "dist/$(basename "$OUT").zip"
(cd target && zip -9r "../dist/$(basename "$OUT").zip" "$(basename "$OUT")")
ls -la dist
