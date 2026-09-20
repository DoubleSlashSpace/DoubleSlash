#!/usr/bin/env bash
# Run the same checks as .github/workflows/ci.yml before pushing.
#
# Mirrors the Linux "Rust tests" job (fmt, clippy, tests, cargo-audit for all
# four Cargo workspaces: rust/, the client, the Android bridge, and the
# supernode manager).
#
# Android extras, skipped when the tools are missing:
#   - Gradle testDebugUnitTest when an Android SDK is configured
#   - cargo ndk clippy of the JNI cdylib when NDK + cargo-ndk + the
#     aarch64-linux-android target are present
#   - assembleDebug only with --include-android-apk
#
# Supernode packaging jobs (linux-x86_64 / aarch64 / win64) run separately in
# CI — use scripts/build_supernode.sh locally when needed.
# Run from the repository root:
#
#   bash scripts/ci_local.sh
#
# Faster iteration (lint only):
#
#   bash scripts/ci_local.sh --skip-tests --skip-audit
#
set -euo pipefail

RUST_TOOLCHAIN="${RUST_TOOLCHAIN:-1.97.1}"
SKIP_TESTS=0
SKIP_AUDIT=0
SKIP_OPUS_FETCH=0
SKIP_ANDROID_NDK=0
INCLUDE_ANDROID_APK=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --skip-tests) SKIP_TESTS=1 ;;
        --skip-audit) SKIP_AUDIT=1 ;;
        --skip-opus-fetch) SKIP_OPUS_FETCH=1 ;;
        --skip-android-ndk) SKIP_ANDROID_NDK=1 ;;
        --include-android-apk) INCLUDE_ANDROID_APK=1 ;;
        --toolchain) RUST_TOOLCHAIN="$2"; shift ;;
        -h|--help)
            echo "Usage: bash scripts/ci_local.sh [--skip-tests] [--skip-audit] [--skip-opus-fetch] [--skip-android-ndk] [--include-android-apk] [--toolchain VERSION]"
            exit 0
            ;;
        *) echo "Unknown option: $1" >&2; exit 2 ;;
    esac
    shift
done

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUST_DIR="$REPO_ROOT/rust"
CLIENT_DIR="$RUST_DIR/doubleslash-client"
ANDROID_RUST_DIR="$RUST_DIR/doubleslash-android"
ANDROID_DIR="$REPO_ROOT/android"
MANAGER_DIR="$RUST_DIR/doubleslash-supernode-manager"

step() {
    echo ""
    echo "==> $1"
}

skip() {
    echo ""
    echo "==> $1"
    echo "    skipped: $2"
}

run_cargo() {
    local dir="$1"
    shift
    (cd "$dir" && cargo "$@")
}

android_sdk_dir() {
    local props="$ANDROID_DIR/local.properties"
    if [[ -f "$props" ]]; then
        local line
        line="$(grep -E '^[[:space:]]*sdk\.dir[[:space:]]*=' "$props" | tail -n1 || true)"
        if [[ -n "$line" ]]; then
            local dir="${line#*=}"
            dir="${dir//$'\r'/}"
            dir="$(echo "$dir" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
            if [[ -d "$dir" ]]; then
                echo "$dir"
                return 0
            fi
        fi
    fi
    if [[ -n "${ANDROID_HOME:-}" && -d "$ANDROID_HOME" ]]; then
        echo "$ANDROID_HOME"
        return 0
    fi
    if [[ -n "${ANDROID_SDK_ROOT:-}" && -d "$ANDROID_SDK_ROOT" ]]; then
        echo "$ANDROID_SDK_ROOT"
        return 0
    fi
    return 1
}

android_ndk_version() {
    grep -E 'ndkVersion[[:space:]]*=' "$ANDROID_DIR/app/build.gradle.kts" \
        | head -n1 \
        | sed -E 's/.*ndkVersion[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/'
}

android_ndk_api() {
    local val
    val="$(grep -E '^doubleslash\.ndkApi=' "$ANDROID_DIR/gradle.properties" | tail -n1 | cut -d= -f2- || true)"
    val="${val//$'\r'/}"
    echo "${val:-26}"
}

echo "DoubleSlash local CI (toolchain $RUST_TOOLCHAIN)"
echo "Repo: $REPO_ROOT"

step "Ensure git submodules (recursive)"
git -C "$REPO_ROOT" submodule update --init --recursive

step "Install Rust $RUST_TOOLCHAIN (rustfmt + clippy)"
rustup toolchain install "$RUST_TOOLCHAIN" --component rustfmt --component clippy
export RUSTUP_TOOLCHAIN="$RUST_TOOLCHAIN"

step "Verify version metadata stays in sync"
if command -v pwsh >/dev/null 2>&1; then
    pwsh "$REPO_ROOT/scripts/check_version_sync.ps1"
elif command -v powershell >/dev/null 2>&1; then
    powershell -ExecutionPolicy Bypass -File "$REPO_ROOT/scripts/check_version_sync.ps1"
else
    echo "pwsh or powershell required for scripts/check_version_sync.ps1" >&2
    exit 1
fi

if [[ "$SKIP_OPUS_FETCH" -eq 0 ]]; then
    step "Fetch Opus DNN model weights"
    bash "$REPO_ROOT/scripts/fetch_opus_weights.sh"
fi

step "cargo fmt --check (rust/ workspace)"
run_cargo "$RUST_DIR" fmt --all -- --check

step "cargo fmt --check (client workspace)"
run_cargo "$CLIENT_DIR" fmt --all -- --check

step "cargo fmt --check (android workspace)"
run_cargo "$ANDROID_RUST_DIR" fmt --all -- --check

step "cargo fmt --check (supernode-manager workspace)"
run_cargo "$MANAGER_DIR" fmt --all -- --check

step "cargo clippy (rust/ workspace, -D warnings)"
run_cargo "$RUST_DIR" clippy --all --all-targets -- -D warnings

step "Release manifest signer self-test"
run_cargo "$RUST_DIR" run -p doubleslash-installer --bin sign-release-manifest -- --self-test

if [[ "$SKIP_TESTS" -eq 0 ]]; then
    step "cargo test --all --release (rust/ workspace)"
    run_cargo "$RUST_DIR" test --all --release

    step "cargo test (client workspace, headless)"
    run_cargo "$CLIENT_DIR" test

    step "cargo test (android workspace, host)"
    run_cargo "$ANDROID_RUST_DIR" test

    step "cargo test (supernode-manager workspace)"
    run_cargo "$MANAGER_DIR" test
fi

step "cargo clippy (client workspace, headless, -D warnings)"
run_cargo "$CLIENT_DIR" clippy -p doubleslash-client --no-default-features --all-targets -- -D warnings

step "cargo clippy (android workspace, host, -D warnings)"
run_cargo "$ANDROID_RUST_DIR" clippy --all-targets -- -D warnings

step "cargo clippy (supernode-manager workspace, -D warnings)"
run_cargo "$MANAGER_DIR" clippy --all-targets -- -D warnings

if [[ "$(uname -s)" != "Darwin" ]]; then
    step "cargo clippy (macOS capture module, cross-linted, -D warnings)"
    run_cargo "$CLIENT_DIR" clippy -p doubleslash-client --no-default-features --features lint-macos -- -D warnings
fi

SDK_DIR=""
if SDK_DIR="$(android_sdk_dir)"; then
    if [[ "$SKIP_TESTS" -eq 0 ]]; then
        step "Gradle testDebugUnitTest (Android SDK present)"
        (
            cd "$ANDROID_DIR"
            export ANDROID_HOME="$SDK_DIR"
            export ANDROID_SDK_ROOT="$SDK_DIR"
            ./gradlew testDebugUnitTest --console=plain --no-daemon
        )
    fi
    if [[ "$INCLUDE_ANDROID_APK" -eq 1 && "$SKIP_TESTS" -eq 0 ]]; then
        step "Gradle assembleDebug (--include-android-apk)"
        (
            cd "$ANDROID_DIR"
            export ANDROID_HOME="$SDK_DIR"
            export ANDROID_SDK_ROOT="$SDK_DIR"
            ./gradlew assembleDebug --console=plain --no-daemon
        )
    elif [[ "$INCLUDE_ANDROID_APK" -eq 1 ]]; then
        skip "Gradle assembleDebug" "--skip-tests is set"
    fi
else
    skip "Gradle Android unit tests" "no Android SDK (local.properties sdk.dir, ANDROID_HOME, or ANDROID_SDK_ROOT)"
fi

NDK_VERSION="$(android_ndk_version)"
NDK_API="$(android_ndk_api)"
NDK_HOME=""
if [[ -n "$SDK_DIR" && -n "$NDK_VERSION" && -d "$SDK_DIR/ndk/$NDK_VERSION" ]]; then
    NDK_HOME="$SDK_DIR/ndk/$NDK_VERSION"
fi

if [[ "$SKIP_ANDROID_NDK" -eq 1 ]]; then
    skip "cargo ndk clippy (aarch64-linux-android)" "--skip-android-ndk is set"
elif [[ -z "$NDK_HOME" ]]; then
    skip "cargo ndk clippy (aarch64-linux-android)" "pinned NDK ${NDK_VERSION:-unknown} not installed under the Android SDK"
elif ! command -v cargo-ndk >/dev/null 2>&1; then
    skip "cargo ndk clippy (aarch64-linux-android)" "cargo-ndk is not on PATH"
elif ! rustup target list --installed | grep -qx 'aarch64-linux-android'; then
    skip "cargo ndk clippy (aarch64-linux-android)" "rustup target aarch64-linux-android is not installed"
else
    step "cargo ndk clippy (arm64-v8a JNI cdylib, -D warnings)"
    (
        cd "$ANDROID_RUST_DIR"
        if [[ -d "$SDK_DIR/cmake/3.31.6/bin" ]]; then
            export PATH="$SDK_DIR/cmake/3.31.6/bin:$PATH"
        fi
        export ANDROID_HOME="$SDK_DIR"
        export ANDROID_NDK_HOME="$NDK_HOME"
        export CARGO_TARGET_DIR="$RUST_DIR/target-android"
        cargo ndk -t arm64-v8a --platform "$NDK_API" clippy --lib -- -D warnings
    )
fi

if [[ "$SKIP_AUDIT" -eq 0 ]]; then
    if ! command -v cargo-audit >/dev/null 2>&1; then
        step "Installing cargo-audit (not on PATH)"
        # Keep in sync with CARGO_AUDIT_VERSION in .github/workflows/ci.yml —
        # latest cargo-audit can require a newer rustc than RUST_TOOLCHAIN.
        cargo install cargo-audit --version 0.22.1 --locked
    fi

    step "cargo audit (rust/ workspace)"
    (cd "$RUST_DIR" && cargo audit --file Cargo.lock)

    step "cargo audit (client workspace)"
    (cd "$CLIENT_DIR" && cargo audit --file Cargo.lock)

    step "cargo audit (android workspace)"
    (cd "$ANDROID_RUST_DIR" && cargo audit --file Cargo.lock)

    step "cargo audit (supernode-manager workspace)"
    (cd "$MANAGER_DIR" && cargo audit --file Cargo.lock)
fi

echo ""
echo "Local CI passed."
