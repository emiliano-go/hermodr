#!/usr/bin/env bash
#
# Restarts the dev server so visual changes show up without rebuilding and
# reinstalling. It stops whatever is running first, so it is safe to run while
# an instance is up.
#
# Usage:
#   pnpm reset        # or: bash scripts/dev-reset.sh
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }

say "stopping the running instance"
# Scoped to this project: the dev server, and our own binaries. Never the
# shared WebKitWebProcess name, which would kill every other WebKit app.
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

say "starting the dev server"
cd "$ROOT"
exec pnpm tauri dev
