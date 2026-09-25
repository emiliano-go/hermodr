#!/usr/bin/env bash
#
# Installs Hermóðr from a GitHub release. It downloads the AppImage, puts it on
# PATH, and adds a desktop entry so it shows up as WhatsApp.
#
#   curl -fsSL https://raw.githubusercontent.com/emiliano-go/hermodr/master/scripts/install.sh | sh
#
# HERMODR_VERSION=v0.1.0 pins a release; the default is the latest one.
#
# The download is pinned to the release's tag and checked against the SHA-256
# digest GitHub computes for the asset, so a corrupted or swapped file aborts
# the install before anything is replaced. There is no signature check: one
# only helps if the signing key lives outside GitHub, which needs a key the
# maintainer holds. Until releases are signed, trust rests on GitHub and the
# repository owner, as it does for the source.
#
set -euo pipefail

REPO="emiliano-go/hermodr"
ASSET="Hermodr-x86_64.AppImage"
API="https://api.github.com/repos/$REPO/releases"

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
fetch() { curl -fsSL --proto '=https' --tlsv1.2 "$@"; }

command -v curl >/dev/null || die "curl is required"
if command -v sha256sum >/dev/null; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  die "sha256sum or shasum is required to verify the download"
fi

BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
TARGET="$BIN_DIR/hermodr"

if [ -n "${HERMODR_VERSION:-}" ]; then
  RELEASE="$(fetch "$API/tags/$HERMODR_VERSION")" || die "release $HERMODR_VERSION not found"
else
  RELEASE="$(fetch "$API/latest")" || die "could not look up the latest release"
fi

TAG="$(printf '%s' "$RELEASE" | grep -oE '"tag_name": *"[^"]*"' | head -n1 | cut -d'"' -f4 || true)"
# Asset names and digests appear in order; the digest after our name is ours.
DIGEST="$(printf '%s' "$RELEASE" | grep -oE '"(name|digest)": *"[^"]*"' \
  | awk -v asset="$ASSET" -F'"' '$2 == "name" { hit = ($4 == asset) } $2 == "digest" && hit { print $4; exit }' || true)"
DIGEST="${DIGEST#sha256:}"

[ -n "$TAG" ] || die "could not read the release tag"
printf '%s' "$DIGEST" | grep -qE '^[0-9a-f]{64}$' || die "release $TAG has no SHA-256 digest for $ASSET"

mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR"
TMP="$(mktemp "$BIN_DIR/.hermodr.XXXXXX")"
trap 'rm -f "$TMP"' EXIT

say "downloading $TAG"
fetch -o "$TMP" "https://github.com/$REPO/releases/download/$TAG/$ASSET"

say "verifying the download"
ACTUAL="$(sha256 "$TMP")"
[ "$ACTUAL" = "$DIGEST" ] || die "checksum mismatch for $ASSET: expected $DIGEST, got $ACTUAL. Nothing was installed."

chmod +x "$TMP"
mv -f "$TMP" "$TARGET"

# `whatsapp` is the name people look for, so provide it as an alias.
ln -sf "$TARGET" "$BIN_DIR/whatsapp"

say "installing the icon and desktop entry"
fetch -o "$ICON_DIR/hermodr.png" \
  "https://raw.githubusercontent.com/$REPO/$TAG/src-tauri/icons/icon.png"

cat > "$APP_DIR/hermodr.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Version=1.0
Name=Hermóðr
GenericName=WhatsApp Client
Comment=Native WhatsApp desktop client
Exec=$TARGET
Icon=hermodr
Terminal=false
Categories=Network;InstantMessaging;Chat;
Keywords=whatsapp;chat;messaging;hermodr;
StartupWMClass=Hermodr
DESKTOP

if command -v update-desktop-database >/dev/null; then
  update-desktop-database "$APP_DIR" || true
fi

say "done: hermodr $TAG installed (also as \"whatsapp\")"
say "make sure $BIN_DIR is on your PATH"
