#!/usr/bin/env bash
# Launch the DoubleSlash native client (debug build).
# Build first: cd rust/doubleslash-client && cargo build --features qt-ui

set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
BINARY="$ROOT/rust/target/debug/doubleslash-client"

if [ ! -f "$BINARY" ]; then
    echo "DoubleSlash client binary not found at:"
    echo "  $BINARY"
    echo ""
    echo "Build it first:"
    echo "  cd rust/doubleslash-client"
    echo "  cargo build --features qt-ui"
    exit 1
fi

exec "$BINARY" "$@"
