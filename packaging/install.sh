#!/usr/bin/env bash
# Build and install Linux Explorer for the current user (default) or system-wide.
#
#   ./packaging/install.sh            # ~/.local
#   sudo ./packaging/install.sh --system   # /usr/local
#   ./packaging/install.sh --default  # also make it the default file manager
#   ./packaging/install.sh --uninstall
#   ./packaging/install.sh --prefix DIR   # any prefix (packaging, testing)
set -euo pipefail

cd "$(dirname "$0")/.."
PREFIX="$HOME/.local"
MAKE_DEFAULT=0
UNINSTALL=0
while [ $# -gt 0 ]; do
  a=$1; shift
  case "$a" in
    --system) PREFIX=/usr/local ;;
    --prefix) PREFIX=$1; shift ;;
    --default) MAKE_DEFAULT=1 ;;
    --uninstall) UNINSTALL=1 ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "unknown option $a" >&2; exit 2 ;;
  esac
done

BIN="$PREFIX/bin/linux-explorer"
DESKTOP="$PREFIX/share/applications/linux-explorer.desktop"
ICON="$PREFIX/share/icons/hicolor/scalable/apps/linux-explorer.svg"

if [ "$UNINSTALL" = 1 ]; then
  rm -f "$BIN" "$DESKTOP" "$ICON"
  echo "Removed Linux Explorer from $PREFIX"
  exit 0
fi

if ! command -v cargo >/dev/null; then
  echo "cargo not found. Install Rust from https://rustup.rs first." >&2
  exit 1
fi

cargo build --release -p linux-explorer
install -Dm755 target/release/linux-explorer "$BIN"
install -Dm644 packaging/linux-explorer.desktop "$DESKTOP"
install -Dm644 packaging/linux-explorer.svg "$ICON"
command -v update-desktop-database >/dev/null && update-desktop-database -q "$(dirname "$DESKTOP")" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true

if [ "$MAKE_DEFAULT" = 1 ]; then
  xdg-mime default linux-explorer.desktop inode/directory
  echo "Linux Explorer is now the default file manager."
fi
echo "Installed to $BIN"
case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) echo "Note: add $PREFIX/bin to your PATH." ;; esac
