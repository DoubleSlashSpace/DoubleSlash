#!/usr/bin/env bash
# ============================================================================
# build_macos.sh — Build DoubleSlash for macOS (Rust + Qt, .app + DMG)
# ============================================================================
# Produces:
#   dist/DoubleSlash.app
#   dist/DoubleSlash-X.X.X-macos-<arch>.dmg  (create-dmg or hdiutil)
#   dist/DoubleSlash-X.X.X.dmg.sha256
#
# Prerequisites:
#   1. Rust toolchain (cargo) on PATH (rustup).
#   2. Qt 6 installed via the Qt installer.
#      Set QT_DIR, e.g.: export QT_DIR=$HOME/Qt/6.8.3/macos
#   3. cmake on PATH (brew install cmake).
#   4. Optional: brew install create-dmg  (nicer DMG layout).
#   5. For distribution, valid Apple Developer certificate + notarytool.
#
# Usage:
#   ./build_macos.sh                    # debug build
#   DOUBLESLASH_RELEASE=1 ./build_macos.sh # release build (optimised)
# ============================================================================

set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
RUST_DIR="$ROOT/rust"
CLIENT_DIR="$RUST_DIR/doubleslash-client"

# ── Auto-detect Qt 6 ─────────────────────────────────────────────────────────
if [ -z "${QT_DIR:-}" ]; then
    for CANDIDATE in \
        "$HOME/Qt/6.8.3/macos" \
        "$HOME/Qt/6.8.*/macos" \
        /opt/homebrew/opt/qt@6 \
        /usr/local/opt/qt@6; do
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

# ── Read version ─────────────────────────────────────────────────────────────
VERSION=$(grep -m1 '^version' "$RUST_DIR/doubleslash-client/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')
echo "==> Building DoubleSlash v${VERSION} for macOS"

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

# ── Assemble .app bundle ───────────────────────────────────────────────────────
DIST="$ROOT/dist"
mkdir -p "$DIST"
APP_BUNDLE="$DIST/DoubleSlash.app"
CONTENTS="$APP_BUNDLE/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"
FRAMEWORKS="$CONTENTS/Frameworks"

echo ""
echo "==> Assembling $APP_BUNDLE..."
rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS" "$RESOURCES" "$FRAMEWORKS"

cp "$BINARY" "$MACOS/doubleslash"
cp "$INSTALLER_BIN" "$MACOS/doubleslash-installer"
chmod +x "$MACOS/doubleslash" "$MACOS/doubleslash-installer"

# Info.plist (from packaging template, with version substitution)
PLIST_TEMPLATE="$ROOT/packaging/Info.plist.in"
if [ -f "$PLIST_TEMPLATE" ]; then
    sed "s/@VERSION@/$VERSION/g" "$PLIST_TEMPLATE" > "$CONTENTS/Info.plist"
else
    cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>              <string>DoubleSlash</string>
  <key>CFBundleIdentifier</key>       <string>com.doubleslash.client</string>
  <key>CFBundleVersion</key>          <string>${VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleExecutable</key>       <string>doubleslash</string>
  <key>CFBundleIconFile</key>         <string>doubleslash.icns</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.social-networking</string>
  <key>NSHighResolutionCapable</key>  <true/>
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLSchemes</key><array><string>doubleslash</string><string>d</string></array>
      <key>CFBundleURLName</key>   <string>DoubleSlash URL</string>
    </dict>
  </array>
</dict>
</plist>
PLIST
fi

# Icon
if [ -f "$ROOT/assets/doubleslash.icns" ]; then
    cp "$ROOT/assets/doubleslash.icns" "$RESOURCES/doubleslash.icns"
fi

# Qt deployment (bundles Qt frameworks + QML runtime)
echo ""
echo "==> Running macdeployqt..."
macdeployqt "$APP_BUNDLE" \
    -qmldir="$RUST_DIR/doubleslash-client/qml" \
    -no-strip

# ── Code signing (optional) ────────────────────────────────────────────────────
SIGN_ID="${DOUBLESLASH_SIGN_ID:-}"
if [ -n "$SIGN_ID" ]; then
    echo "==> Code signing with identity: $SIGN_ID"
    ENTITLEMENTS="$ROOT/packaging/doubleslash.entitlements"
    codesign --deep --force --sign "$SIGN_ID" \
             --entitlements "$ENTITLEMENTS" \
             "$APP_BUNDLE"
else
    echo "==> Skipping code signing (DOUBLESLASH_SIGN_ID not set)"
fi

# ── Create DMG ────────────────────────────────────────────────────────────────
ARCH="$(uname -m)"
case "$ARCH" in
    arm64)  PLATFORM_SUFFIX="macos-arm64" ;;
    x86_64) PLATFORM_SUFFIX="macos-x86_64" ;;
    *)      PLATFORM_SUFFIX="macos-${ARCH}" ;;
esac
DMG="$DIST/DoubleSlash-${VERSION}-${PLATFORM_SUFFIX}.dmg"

create_dmg_with_hdiutil() {
    echo "==> Creating DMG with hdiutil..."
    hdiutil create -volname "DoubleSlash $VERSION" \
        -srcfolder "$APP_BUNDLE" \
        -ov -format UDZO \
        "$DMG"
}

echo ""
if command -v create-dmg &>/dev/null; then
    echo "==> Creating DMG with create-dmg..."
    CREATE_DMG_ARGS=(
        --volname "DoubleSlash $VERSION"
        --window-pos 200 120
        --window-size 600 400
        --icon-size 100
        --icon "DoubleSlash.app" 175 190
        --hide-extension "DoubleSlash.app"
        --app-drop-link 425 190
    )
    if [ -f "$ROOT/assets/dmg_background.png" ]; then
        CREATE_DMG_ARGS+=(--background "$ROOT/assets/dmg_background.png")
    else
        echo "    (no assets/dmg_background.png — using default window)"
    fi
    if ! create-dmg "${CREATE_DMG_ARGS[@]}" "$DMG" "$DIST"; then
        echo "==> create-dmg failed; falling back to hdiutil..."
        create_dmg_with_hdiutil
    fi
else
    create_dmg_with_hdiutil
fi

if [ ! -f "$DMG" ]; then
    echo "ERROR: DMG was not created at $DMG"
    exit 1
fi

# ── Notarization (optional) ────────────────────────────────────────────────────
if [ -n "${DOUBLESLASH_APPLE_ID:-}" ] && [ -n "${DOUBLESLASH_APPLE_TEAM_ID:-}" ]; then
    echo "==> Submitting for notarization..."
    xcrun notarytool submit "$DMG" \
        --apple-id "$DOUBLESLASH_APPLE_ID" \
        --team-id  "$DOUBLESLASH_APPLE_TEAM_ID" \
        --password "$DOUBLESLASH_APPLE_APP_PASSWORD" \
        --wait
    xcrun stapler staple "$DMG"
else
    echo "==> Skipping notarization (DOUBLESLASH_APPLE_ID not set)"
fi

# ── Checksum ───────────────────────────────────────────────────────────────────
if [ -f "$DMG" ]; then
    shasum -a 256 "$DMG" | tee "${DMG}.sha256"
    echo ""
    echo "==> Done: $DMG"
fi
