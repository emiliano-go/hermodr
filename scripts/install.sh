#!/usr/bin/env bash
#
# Installs Postal from the tip of the default branch. Releases are on hold
# until they cover every platform Postal supports, so this fetches the source,
# builds it on this machine and installs the result.
#
#   curl -fsSL https://raw.githubusercontent.com/emiliano-go/postal/master/scripts/install.sh | sh
#
# POSTAL_REF pins a branch, tag or commit (default: master). Arguments go to
# scripts/install-dev.sh, so a quicker unoptimized build is:
#
#   curl -fsSL .../install.sh | sh -s -- --debug
#
# The build needs Rust, Node with pnpm, and the system packages Tauri wants;
# scripts/install-dev.sh prints what is missing. Nothing is looked up over the
# GitHub API and no prebuilt artifact is trusted: the code is built here.
#
set -euo pipefail

REPO="emiliano-go/postal"
REF="${POSTAL_REF:-master}"
TARBALL="https://codeload.github.com/$REPO/tar.gz/$REF"

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
fetch() { curl -fsSL --proto '=https' --tlsv1.2 "$@"; }

command -v curl >/dev/null || die "curl is required"
command -v tar >/dev/null || die "tar is required"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/postal-install.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

say "fetching $REPO@$REF"
fetch "$TARBALL" | tar -xz -C "$WORK"

# codeload wraps the tree in a single directory named after the ref.
ROOT=""
for candidate in "$WORK"/*/; do
  ROOT="${candidate%/}"
  break
done
[ -n "$ROOT" ] && [ -x "$ROOT/scripts/install-dev.sh" ] || die "the archive did not look like Postal"

# An install should be optimized; --debug is there for a quick round trip.
ARGS=("$@")
if [ "${#ARGS[@]}" -eq 0 ]; then
  ARGS=(--release)
fi
"$ROOT/scripts/install-dev.sh" "${ARGS[@]}"
