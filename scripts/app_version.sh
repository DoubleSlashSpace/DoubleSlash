#!/usr/bin/env bash
# Print the product version: {major}.{minor}.{build}.0
# major.minor come from rust/doubleslash-client/Cargo.toml.
# build is DOUBLESLASH_BUILD_NUMBER, or git rev-list --count HEAD.
# The fourth component stays 0 so this string is also the Microsoft Store identity.
# A shallow clone is an error here. Build scripts stamp 0 in that case; packaging must not.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cargo="$root/rust/doubleslash-client/Cargo.toml"
line="$(grep -m1 '^version' "$cargo")"
if [[ ! "$line" =~ version[[:space:]]*=[[:space:]]*\"([0-9]+)\.([0-9]+)\.[0-9]+\" ]]; then
    echo "Cannot read the client release version from $cargo" >&2
    exit 1
fi
major="${BASH_REMATCH[1]}"
minor="${BASH_REMATCH[2]}"
if (( major < 1 || major > 65535 || minor > 65535 )); then
    echo "Client version $major.$minor is outside the Windows version range." >&2
    exit 1
fi
if [[ -n "${DOUBLESLASH_BUILD_NUMBER:-}" ]]; then
    build="$DOUBLESLASH_BUILD_NUMBER"
else
    shallow="$(git -C "$root" rev-parse --is-shallow-repository)"
    if [[ "$shallow" == "true" ]]; then
        echo "Shallow checkout. Fetch full history or set DOUBLESLASH_BUILD_NUMBER." >&2
        exit 1
    fi
    build="$(git -C "$root" rev-list --count HEAD)"
fi
if (( build < 0 || build > 65535 )); then
    echo "Build number $build exceeds the 65535 limit of Windows and Microsoft Store versions." >&2
    exit 1
fi
printf '%s.%s.%s.0\n' "$major" "$minor" "$build"
