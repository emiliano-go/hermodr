#!/usr/bin/env bash
#
# Builds and installs Postal from the current checkout.
#
# Dependencies are declared in Cargo.toml: Tauri comes from crates.io, and
# whatsapp-rust is pinned to a git revision (the per-chunk history-sync control
# it needs is newer than the last release). Cargo fetches both, so there is
# nothing to clone by hand.
#
# Usage:
#   scripts/install-dev.sh          # build a release and install it
#   scripts/install-dev.sh --dev    # start the dev server
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

for tool in cargo node pnpm; do
  command -v "$tool" >/dev/null || die "$tool is required but not on PATH"
done

say "stopping any running instance"
# Scoped to this project: never the shared WebKitWebProcess name, which would
# kill every other WebKit app (Zuno, opencode, and so on).
stop_app() {
  pkill -9 -f "$ROOT/node_modules" 2>/dev/null || true
  local pid
  for pid in $(pgrep -f "$ROOT/src-tauri/target/(debug|release)/(postal|hermodr)|$HOME/.local/bin/(postal|hermodr)|/\.mount_[^/]*/(AppRun|usr/bin/(postal|hermodr))" 2>/dev/null); do
    pkill -9 -P "$pid" 2>/dev/null || true
    kill -9 "$pid" 2>/dev/null || true
  done
}
stop_app
sleep 1

say "installing node dependencies"
(cd "$ROOT" && pnpm install)

if [ "${1:-}" = "--dev" ]; then
  say "starting the dev server"
  cd "$ROOT" && exec pnpm tauri dev
fi

# Debug by default: it embeds the frontend the same way, but builds in a
# fraction of the time, which is what a local install wants. Pass --release for
# the optimized binary. `--no-bundle` skips the AppImage tooling.
profile="debug"
flag="--debug"
if [ "${1:-}" = "--release" ] || [ "${2:-}" = "--release" ]; then
  profile="release"
  flag=""
fi
say "building ($profile)"
(cd "$ROOT" && pnpm tauri build --no-bundle $flag)

say "installing the desktop entry"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
BIN="$ROOT/src-tauri/target/$profile/postal"
[ -x "$BIN" ] || die "release binary not found at $BIN"
mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR"
install -m 755 "$BIN" "$BIN_DIR/postal"
# `whatsapp` is the name people look for, so provide it as an alias.
ln -sf "$BIN_DIR/postal" "$BIN_DIR/whatsapp"
# Drop the pre-rename install so only one entry remains in the app menu.
rm -f "$BIN_DIR/hermodr" "$APP_DIR/hermodr.desktop" "$ICON_DIR/hermodr.png"
install -m 644 "$ROOT/src-tauri/icons/icon.png" "$ICON_DIR/postal.png"
cat > "$APP_DIR/postal.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Version=1.0
Name=Postal
GenericName=WhatsApp Client
Comment=Native WhatsApp desktop client
Exec=$BIN_DIR/postal
Icon=postal
Terminal=false
Categories=Network;InstantMessaging;Chat;
Keywords=whatsapp;chat;messaging;postal;
StartupWMClass=postal
DESKTOP
if command -v update-desktop-database >/dev/null; then
  update-desktop-database "$APP_DIR" || true
fi

say "done: postal installed (also as \"whatsapp\")"
say "make sure $BIN_DIR is on your PATH"

# System packages are not installed here on purpose. What the build needs beyond
# the three tools above:
#
#   Arch       webkit2gtk-4.1 gtk3 libappindicator-gtk3 librsvg pkgconf openssl
#   Debian     libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
#              librsvg2-dev libssl-dev pkg-config build-essential
