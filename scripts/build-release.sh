#!/usr/bin/env bash
#
# Builds the release bundles, AppImage included.
#
# Two environment variables are needed on current distros, because the
# AppImage tooling is old:
#
#   NO_STRIP=1                linuxdeploy's bundled strip cannot read the
#                             `.relr.dyn` sections modern binutils emits, so
#                             it fails on the bundled libraries.
#   APPIMAGE_EXTRACT_AND_RUN=1 linuxdeploy ships as an AppImage and needs
#                             libfuse2 to run, which is often not installed.
#                             This makes it self extract instead.
#
# Usage:
#   scripts/build-release.sh                       # all bundles
#   scripts/build-release.sh --bundles appimage    # just the AppImage
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null || die "cargo is required but not on PATH"
command -v pnpm >/dev/null || die "pnpm is required but not on PATH"

export NO_STRIP=1
export APPIMAGE_EXTRACT_AND_RUN=1

say "building the release bundles"
cd "$ROOT"
pnpm tauri build "$@"

# The files to upload to the GitHub release, named as scripts/install.sh expects,
# with the SHA256SUMS it verifies them against.
APPIMAGE="$(ls -t src-tauri/target/release/bundle/appimage/*.AppImage 2>/dev/null | head -n1 || true)"
if [ -n "$APPIMAGE" ]; then
  OUT="$ROOT/dist"
  mkdir -p "$OUT"
  cp "$APPIMAGE" "$OUT/Hermodr-x86_64.AppImage"
  cp src-tauri/icons/icon.png "$OUT/hermodr.png"
  (cd "$OUT" && sha256sum Hermodr-x86_64.AppImage hermodr.png > SHA256SUMS)
  say "release assets in $OUT: upload Hermodr-x86_64.AppImage, hermodr.png and SHA256SUMS"
fi
