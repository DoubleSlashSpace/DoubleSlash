#!/usr/bin/env bash
# ============================================================================
# install_uri_scheme.sh — Register the DoubleSlash URI schemes on Linux
# ============================================================================
# Installs the .desktop file and registers it as the handler for
# doubleslash:// and d:// URLs so clicking invite links launches DoubleSlash.
#
# Usage:
#   ./packaging/install_uri_scheme.sh          # current user only
#   sudo ./packaging/install_uri_scheme.sh     # system-wide
# ============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DESKTOP_FILE="$SCRIPT_DIR/doubleslash.desktop"

if [ ! -f "$DESKTOP_FILE" ]; then
    echo "ERROR: doubleslash.desktop not found at $DESKTOP_FILE"
    exit 1
fi

if [ "$(id -u)" -eq 0 ]; then
    DEST="/usr/share/applications"
    echo "Installing doubleslash.desktop system-wide to $DEST..."
else
    DEST="$HOME/.local/share/applications"
    mkdir -p "$DEST"
    echo "Installing doubleslash.desktop for current user to $DEST..."
fi

cp "$DESKTOP_FILE" "$DEST/doubleslash.desktop"
update-desktop-database "$DEST" 2>/dev/null || true

xdg-mime default doubleslash.desktop x-scheme-handler/doubleslash 2>/dev/null || true
xdg-mime default doubleslash.desktop x-scheme-handler/d 2>/dev/null || true

echo "Done. The doubleslash:// and d:// URI schemes are now registered."
echo "Test with: xdg-open 'doubleslash://test'"
