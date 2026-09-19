#!/usr/bin/env bash
# ============================================================================
# build_linux.sh — Build DoubleSlash for Linux (Rust + Qt, AppImage)
# ============================================================================
# Produces:
#   dist/DoubleSlash-X.X.X-x86_64.AppImage
#   dist/DoubleSlash-X.X.X-x86_64.AppImage.sha256
#
# Prerequisites:
#   1. Rust toolchain (cargo) on PATH.
#   2. Qt 6 installed; set QT_DIR or CMAKE_PREFIX_PATH.
#      e.g. export QT_DIR=/opt/Qt/6.8.3/gcc_64
#   3. linuxdeployqt or appimagetool on PATH.
#      linuxdeployqt: https://github.com/probonopd/linuxdeployqt/releases
#      appimagetool:  https://github.com/AppImage/appimagetool/releases
#   4. cmake on PATH.
#
# System packages (Debian/Ubuntu):
#   sudo apt install build-essential cmake lld libgl-dev libxkbcommon-dev \
#                    libdbus-1-dev libpulse-dev libminiupnpc-dev fuse
#
#   `lld` is not optional on a machine that also has GNU gold. cxx-qt's
#   qt-build-utils picks a linker itself when the system `ld` is bfd, in the
#   order lld > gold > mold, and appends `-fuse-ld=` to the link line. Gold
#   extracts archive members only during its one sequential scan, and rustc
#   places the rlib defining `cxx_qt_init_*` before the --whole-archive
#   call-init archives that reference them, so gold ends the link with
#   "undefined reference to 'cxx_qt_init_crate_doubleslash_client'". lld keeps
#   archive members as lazy symbols and resolves them whatever the order.
#
# Usage:
#   ./build_linux.sh             # debug build
#   DOUBLESLASH_RELEASE=1 ./build_linux.sh  # release build (optimised)
# ============================================================================

set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
RUST_DIR="$ROOT/rust"
CLIENT_DIR="$RUST_DIR/doubleslash-client"

# ── Auto-detect Qt 6 ─────────────────────────────────────────────────────────
if [ -z "${QT_DIR:-}" ]; then
    for CANDIDATE in \
        /opt/Qt/6.8.3/gcc_64 \
        /opt/Qt/6.8.*/gcc_64 \
        "$HOME/Qt/6.8.3/gcc_64" \
        "$HOME/Qt/6.8.*/gcc_64" \
        /usr/local/Qt-6.8.3; do
        if [ -f "$CANDIDATE/bin/qmake" ]; then
            QT_DIR="$CANDIDATE"
            break
        fi
    done
fi
if [ -z "${QT_DIR:-}" ]; then
    echo "ERROR: Qt 6 not found. Set QT_DIR to the Qt installation root."
    exit 1
fi
echo "==> Using Qt: $QT_DIR"
export PATH="$QT_DIR/bin:$PATH"
export CMAKE_PREFIX_PATH="$QT_DIR"
export QMAKE="$QT_DIR/bin/qmake"

# ── Read version from Cargo.toml ─────────────────────────────────────────────
VERSION=$(grep -m1 '^version' "$RUST_DIR/doubleslash-client/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')
echo "==> Building DoubleSlash v${VERSION} for Linux"

PROFILE="debug"
CARGO_FLAGS=""
if [ "${DOUBLESLASH_RELEASE:-0}" = "1" ]; then
    PROFILE="release"
    CARGO_FLAGS="--release"
fi

# ── Build ─────────────────────────────────────────────────────────────────────
# doubleslash-client is its own workspace root (see rust/doubleslash-client/Cargo.toml).
echo ""
echo "==> cargo build --features qt-ui $CARGO_FLAGS  (doubleslash-client workspace)"
cd "$CLIENT_DIR"
cargo build --features qt-ui $CARGO_FLAGS

echo ""
echo "==> cargo build -p doubleslash-installer $CARGO_FLAGS"
cd "$RUST_DIR"
cargo build -p doubleslash-installer $CARGO_FLAGS

BINARY="$RUST_DIR/target/$PROFILE/doubleslash-client"
INSTALLER_BIN="$RUST_DIR/target/$PROFILE/doubleslash-installer"

# ── Assemble AppDir ───────────────────────────────────────────────────────────
DIST="$ROOT/dist"
APPDIR="$DIST/DoubleSlash.AppDir"
echo ""
echo "==> Assembling AppDir at $APPDIR..."

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"

cp "$BINARY" "$APPDIR/usr/bin/doubleslash"
cp "$INSTALLER_BIN" "$APPDIR/usr/bin/doubleslash-installer"
chmod +x "$APPDIR/usr/bin/doubleslash" "$APPDIR/usr/bin/doubleslash-installer"

TARGET="$(rustc -vV | sed -n 's/^host: //p')"
node "$ROOT/scripts/generate_licenses.mjs" --product client --target "$TARGET" \
    --features qt-ui --output "$APPDIR/usr/share/doubleslash/licenses/client"
node "$ROOT/scripts/generate_licenses.mjs" --product installer --target "$TARGET" \
    --output "$APPDIR/usr/share/doubleslash/licenses/installer"
cp "$ROOT/LICENSE" "$APPDIR/usr/share/doubleslash/LICENSE.txt"

# Desktop integration
cp "$ROOT/packaging/AppRun" "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"
cp "$ROOT/packaging/doubleslash.desktop" "$APPDIR/usr/share/applications/doubleslash.desktop"
ln -sf usr/share/applications/doubleslash.desktop "$APPDIR/doubleslash.desktop"

# Icon (PNG)
ICON_SRC=""
if [ -f "$ROOT/assets/doubleslash_256.png" ]; then
    ICON_SRC="$ROOT/assets/doubleslash_256.png"
fi
if [ -n "$ICON_SRC" ]; then
    cp "$ICON_SRC" "$APPDIR/doubleslash.png"
    cp "$ICON_SRC" "$APPDIR/usr/share/icons/hicolor/256x256/apps/doubleslash.png"
elif [ -f "$ROOT/assets/doubleslash.ico" ]; then
    ICO_SRC="$ROOT/assets/doubleslash.ico"
    if command -v convert &>/dev/null; then
        convert "$ICO_SRC[0]" "$APPDIR/doubleslash.png"
        cp "$APPDIR/doubleslash.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/doubleslash.png"
    fi
fi

# Qt libraries
echo ""
echo "==> Running linuxdeployqt..."
LDQT="${LINUXDEPLOYQT:-$(command -v linuxdeployqt 2>/dev/null || true)}"
if [ -n "$LDQT" ] && [ -f "$LDQT" ]; then
    "$LDQT" "$APPDIR/usr/bin/doubleslash" -appimage \
        -qmldir="$RUST_DIR/doubleslash-client/qml" \
        -no-translations
else
    # linuxdeployqt not found — copy Qt libs manually + use windeployqt-style copy
    echo "  WARNING: linuxdeployqt not found; Qt libraries not bundled automatically."
    echo "  Install from: https://github.com/probonopd/linuxdeployqt/releases"
fi

# ── Package as AppImage ───────────────────────────────────────────────────────
APPIMAGETOOL="${APPIMAGETOOL:-$(command -v appimagetool 2>/dev/null || true)}"
if [ -z "$APPIMAGETOOL" ]; then
    echo ""
    echo "WARNING: appimagetool not found; skipping AppImage packaging."
    echo "Install from: https://github.com/AppImage/appimagetool/releases"
else
    ARCH="x86_64"
    APPIMAGE="$DIST/DoubleSlash-${VERSION}-${ARCH}.AppImage"
    echo ""
    echo "==> Creating AppImage: $APPIMAGE"
    ARCH="$ARCH" "$APPIMAGETOOL" "$APPDIR" "$APPIMAGE"
    node "$ROOT/scripts/licenses/verify_artifact.mjs" "$APPIMAGE" client installer
    sha256sum "$APPIMAGE" | tee "${APPIMAGE}.sha256"
    echo "==> Done: $APPIMAGE"
fi
