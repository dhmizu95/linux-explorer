#!/usr/bin/env bash
# Render the main UI surfaces to PNGs for a quick visual review.
#   scripts/visual-check.sh [OUT_DIR]
# Uses a throwaway demo folder and config, never your real settings.
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=${1:-target/visual-check}
mkdir -p "$OUT"
cargo build -p linux-explorer
BIN=${CARGO_TARGET_DIR:-target}/debug/linux-explorer

DEMO=$(mktemp -d)
trap 'rm -rf "$DEMO"' EXIT
mkdir -p "$DEMO"/{Projects,Photos,"Tax Documents"}
printf 'notes\n' > "$DEMO/notes.txt"
printf 'fn main() {}\n' > "$DEMO/main.rs"
head -c 200000 /dev/urandom > "$DEMO/report2.pdf"
head -c 90000 /dev/urandom > "$DEMO/report10.pdf"
head -c 2000000 /dev/urandom > "$DEMO/backup.zip"
ln -s "$DEMO/notes.txt" "$DEMO/notes - Shortcut"
export XDG_CONFIG_HOME="$DEMO/.config"

shot() {
  local name=$1; shift
  timeout 40 "$BIN" "$@" --size 1280x800 --screenshot "$OUT/$name.png" >/dev/null 2>&1 || echo "FAILED: $name"
}
shot details      "$DEMO" --theme light --select-first
shot details-dark "$DEMO" --theme dark --select-first
shot large-icons  "$DEMO" --theme light --view large
shot tiles        "$DEMO" --theme light --view tiles --preview
shot context-menu "$DEMO" --theme light --select-first --test-action menu-items
shot properties   "$DEMO" --theme light --select-first --test-action properties
shot options      "$DEMO" --theme light --test-action options
shot rename       "$DEMO" --theme light --select-first --test-action rename
shot this-pc      thispc  --theme light
shot home         home    --theme light
ls -1 "$OUT"
