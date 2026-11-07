#!/usr/bin/env bash
# Fresh install: uninstall, wipe saved settings, rebuild from scratch, install.
# Extra args go to install.sh (e.g. --default, --system, --prefix DIR).
#   ./packaging/clean-install.sh [--keep-settings] [install.sh options]
set -euo pipefail
cd "$(dirname "$0")/.."

KEEP=0
ARGS=()
for a in "$@"; do
  if [ "$a" = --keep-settings ]; then KEEP=1; else ARGS+=("$a"); fi
done

# --uninstall must target the same prefix as the install, so reuse the args minus --default.
UNARGS=()
for a in "${ARGS[@]}"; do [ "$a" = --default ] || UNARGS+=("$a"); done
./packaging/install.sh "${UNARGS[@]}" --uninstall

if [ "$KEEP" = 0 ]; then
  rm -rf "${XDG_CONFIG_HOME:-$HOME/.config}/linux-explorer"
  echo "Removed saved settings"
fi

cargo clean -p linux-explorer -p explorer-core
./packaging/install.sh "${ARGS[@]}"
