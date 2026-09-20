# Dependency Licensing and Distribution

The [MIT license](../LICENSE) covers DoubleSlash's own code. It does not
replace the licenses of dependencies, fonts, codecs, runtime libraries, or
other bundled assets. The MIT declarations in the Opus and VP8 wrapper
manifests cover the wrappers, not the vendored upstream libraries.

## Automated Checks

[Dependency Licenses](../.github/workflows/licenses.yml) checks all four Rust
workspaces with `cargo-deny` and generates notices for the current release
targets with `cargo-about`. Release and Android workflows require this gate;
the scheduled supply-chain workflow also runs advisory, ban, and source checks.
Repository branch protection must separately require this workflow if merging
without a successful license check is to be prohibited.

Install Node.js 20 or newer and the pinned tools before packaging:

```sh
cargo install cargo-about --version 0.8.4 --locked
cargo install cargo-deny --version 0.19.0 --locked
node --test scripts/licenses/*.test.mjs
```

The [generator](../scripts/generate_licenses.mjs) resolves a product's locked
Cargo graph from its own workspace, filtered by target and features. It fails
on unresolved or unaccepted license expressions. Build-only and test-only
dependencies are excluded from notices, but remain in the policy check. Code
or assets copied into an executable by a build script need separate review.

To regenerate one product's notices:

```sh
node scripts/generate_licenses.mjs --product client --target x86_64-pc-windows-msvc --features qt-ui,webengine --output dist/license-audit/client
```

Generated directories contain readable Rust license texts, the project license,
an inventory of crate versions, the target/features, and the Cargo lockfile
SHA-256. Client and Android directories also contain the vendored Opus and
libvpx license and patent notices. Source links identify exact crates.io
archives, or the public repository at the revision that was built for
first-party and patched crates. Verify source accessibility when distributing.

The policies include font licenses for the installer's embedded fonts and the
CA-root data license. Permission to bundle a font does not permit relicensing
it under MIT, selling it alone where prohibited, or ignoring reserved-name
conditions. An allowed SPDX identifier is not a determination that every
condition has been satisfied.

## Supplement Notices

Rust dependencies are handled automatically. Everything else a package bundles
- Qt, Chromium, FFmpeg, Mesa, the MSVC redistributables - has its notices kept
as plain text under `packaging/licenses/<target>/<product>/`. The generator
finds that directory by target and product and copies it into the package as
`licenses/<product>/supplement/`, so adding a file there ships it and no build
needs to be told where to look. Android has no such components yet, so
`packaging/licenses/<target>/android/` does not exist.

Nothing checks that the set is complete. That judgement is yours, and it is
worth making before a release:

Review the exact binaries and assets that will be shipped, including:

- Qt modules and plugins, Chromium/WebEngine, FFmpeg and other native runtime
  dependencies actually bundled by the Qt deployment tools.
- Applicable Qt commercial terms or open-source terms, license and copyright
  notices, exact corresponding source availability, and LGPL replacement or
  relinking requirements. Dynamic linking alone is not complete compliance.
- Android's resolved JVM dependencies, their NOTICE files, Oboe, the NDK C++
  runtime, and any code embedded by native build scripts. Review both debug
  and release dependency sets if distributing both.
- Fonts, icons, sounds, game/portal dependencies, and Opus model data. Do not
  infer ownership or patent clearance from a filename or a Cargo manifest.

Qt SBOMs help identify components but may contain `NOASSERTION`; do not use
them as a substitute for license texts or source obligations. See the official
[Qt licensing guidance](https://doc.qt.io/qt-6/licensing.html) and
[WebEngine guidance](https://doc.qt.io/qt-6/qtwebengine-licensing.html), using the
documentation and source corresponding to the Qt version actually shipped.
Preserve available SBOMs alongside the review.

### Windows Evidence Collection

The project owner selected the open-source Qt licensing route and confirmed on
2026-09-16 that the project's artwork, icons, and sounds are original. This does
not cover third-party assets embedded in dependencies or establish compliance
with Qt's distribution obligations.

Collect reproducible evidence from an existing Windows bundle:

```powershell
node scripts/collect_qt_license_evidence.mjs --qt C:/Qt/6.8.3/msvc2022_64 --bundle dist/DoubleSlash --output dist/license-audit/windows-native
```

The [collector](../scripts/collect_qt_license_evidence.mjs) records SHA-256 hashes,
file-level licensing metadata, package sources, and unresolved SBOM entries. It
copies relevant upstream SBOMs but never generates a distribution approval.
It distinguishes raw SBOM checksum matches, reconstructed pre-signing PE matches,
and exact matches to installed Qt files at SBOM paths. Only the first two
reproduce an upstream checksum; none substitutes for a complete license review.
It inventories DLL/EXE files only, not embedded dependencies or other assets.

The inspected local bundle had 110 DLL/EXE files. All 98 matched Qt binaries
reproduce their upstream pre-signing checksums after removing the trailing
Authenticode certificate and zeroing the certificate directory and PE checksum.
Signing explains the earlier discrepancy for these files. See the retained
[technical evidence and compressed SBOMs](../packaging/licenses/evidence/windows-qt-6.8.3/README.md).
The unmatched set consisted of the two DoubleSlash executables, QtWebEngineProcess,
five FFmpeg DLLs, D3Dcompiler_47, dxcompiler, opengl32sw, and vc_redist.x64.
These are investigation results for that local bundle. Nothing in the build or
release path consumes them, and a matched Qt DLL says nothing about Chromium's
embedded third-party notices, which are a separate requirement recorded under
[Known Gaps](#known-gaps).

To enumerate Android's resolved JVM artifacts, their POMs, nested JAR notices
and the NDK notices when writing a component list, use Gradle directly:

```sh
./gradlew :app:dependencies --configuration debugRuntimeClasspath
```

### What this does and does not do

Notices are generated from the resolved dependency graph and copied from
`packaging/licenses/`, and `verify_artifact.mjs` confirms afterwards that they
actually reached the built archive and are readable. That is the whole of it.

There is no reviewer signature, no build-input binding, no per-binary hash
inventory and no gate that refuses to package: none of that is a licence
obligation, and all of it cost more than it caught. No tool verifies that the
component set is complete or that a conclusion about a licence is correct.

These files are public distribution materials, not credentials.

<a id="known-gaps"></a>

## Notice Coverage and Gaps

Everything below was checked against the tree on 2026-09-19. These are
technical findings, not legal advice.

The two component sets that had no notice — the Chromium snapshot's
third-party credits and the Android native and JVM dependencies — are closed,
each verified inside a built artifact rather than a staging directory. What
remains is acceptance on platforms this machine cannot build.

### Chromium third-party credits — closed 2026-09-19

`chromium-122-LICENSE.txt` is only Chromium's own top-level BSD-3-Clause
licence. The credits set for the components bundled inside the Chromium
snapshot — the list a Chrome build exposes at `chrome://credits` — now ships
beside it as `chromium-122-third-party-credits.txt`: 237 components with their
full licence texts (183 distinct), 12 directories that are Chromium's own code
under the licence above, and 55 components whose licence file Qt strips from
its snapshot and which are therefore not compiled into Qt WebEngine.

It could not be extracted from anything Qt ships. Qt WebEngine builds only
Chromium's content layer, so the credits resource is not compiled in — parsing
all four `resources/*.pak` files in the 6.8.3 install (1741 resources) found no
credits markers. Qt's per-component attribution pages exist only for releases
`doc.qt.io` still serves, and the archives jump from 6.7 to 6.9, so there is no
6.8 list; a newer release's component set must not be passed off as this
build's. `qtattributionsscanner` ships in the Qt bin directory but reads
`qt_attribution.json` from a Qt source tree, and the binary install has neither
a source tree nor any `.qch`.

So it comes from the source Qt pins.
[scripts/licenses/chromium_credits.py](../scripts/licenses/chromium_credits.py)
resolves `qtwebengine`'s `src/3rdparty` submodule for a given tag — v6.8.3
pins `55749ed0af5869215b88007df0cba430746583ae` — then takes a blobless,
depth-1, sparse clone that fetches the 488 `README.chromium` files, reads which
licence files they reference, and fetches exactly those. That is about 20 MB
rather than the several GB a full Chromium checkout costs, so it runs in a
couple of minutes on a normal connection.

Two details in that script are worth keeping:

- `README.chromium` values wrap onto indented continuation lines (FreeType's
  `License:` does), and a free-text description follows further down, so the
  header cannot be parsed by stopping at the first blank or unmatched line.
- A `License File:` value beginning `//` is relative to the Chromium source
  root, not to the component directory. Most of the vendored Rust crates use
  that form.

Re-run it whenever the bundled Qt version changes; the component set and the
Chromium revision both move with it.

### Android notices — closed 2026-09-19

Two things the APK distributes had no notice. Both are now covered by an
Android supplement under `packaging/licenses/<target>/android/`, staged for
all four ABIs the Gradle build maps, and confirmed present in a built APK by
`verify_artifact.mjs`.

**The C++ runtime.** The APK statically links the NDK's `libc++_static` and
`libc++abi` into its one native library, and neither has a Rust crate to carry
its notice (see `rust/doubleslash-android/build.rs`).

- `llvm-libcxx-NOTICE.txt` — the LLVM-only notice copied unmodified from the
  NDK's `toolchains/llvm/prebuilt/<host>/NOTICE`.
- `ndk-cxx-runtime-source-information.txt` — what is linked, the NDK revision,
  why the LLVM Exception matters for static linking, and upstream sources.

The NDK's top-level `NOTICE.toolchain` is deliberately **not** used. It covers
the whole toolchain including the GNU tools and opens with the GPL, none of
which is linked into or distributed with this APK; shipping it would assert
something untrue about the package.

**The JVM dependencies.** AndroidX, Compose, CameraX, the Kotlin standard
library and coroutines are packaged as dex inside the APK. The Android Gradle
Plugin's default packaging rules exclude META-INF license and notice files, so
the `merges` rule in `android/app/build.gradle.kts` does not put upstream
notices in the package: an inspected build had 58 META-INF entries, all
`.version` markers and not one LICENSE or NOTICE. Without a supplement these
97 artifacts shipped with no licence text at all.

- `jvm-dependencies-source-information.txt` — all 97 resolved artifacts with
  version and declared licence, read from each POM in the Gradle module cache,
  plus how to regenerate the list.
- `apache-2.0.txt` — covers every artifact but one.
- `libyuv-LICENSE.txt` — `androidx.camera:camera-core` declares BSD alongside
  Apache-2.0 because it embeds libyuv, whose terms require its copyright
  notice and disclaimer be reproduced in binary redistributions.

That list is pinned to the versions in `gradle/libs.versions.toml`. Changing or
adding a dependency means regenerating it; nothing detects a stale entry.

**Oboe and the NDK crates are not a gap.** `oboe`, `oboe-sys`, `ndk`,
`ndk-sys`, `jni` and `cpal` are Rust dependencies and a generated Android
inventory carries all six with their licence texts; `oboe-sys` vendors and
builds the C++ Oboe library under the same Apache-2.0 the crate declares.

### Acceptance

Done on 2026-09-19:

- **Android APK.** A built `app-debug.apk` was inspected and passed
  `node scripts/licenses/verify_artifact.mjs <apk> android`: one inventory,
  all five supplement notices present and non-empty.

Still open:

- **Desktop release archives.** `verify_artifact.mjs` runs automatically at
  the end of `build_win64.ps1`, `build_linux.sh` and `build_macos.sh`, so this
  is a matter of running each one and keeping the output. A Windows release
  build on 2026-09-19 compiled, generated notices and deployed the Qt/WebEngine
  runtime, then stopped at the publish step because a running client held
  `dist\DoubleSlash`; the archive and verification steps come after publish and
  did not run. Re-run with the client closed.
- **Android device acceptance** of the Legal reader.
- **Publication paths** (stable, nightly, signed, unsigned, SignPath restore,
  installer update, supernode deployment) against real installations.

### Not gaps

Recorded because they were previously tracked as blockers and are not:

- **SBOM checksum provenance.** The reconstructed pre-signing PE hashes, the
  twelve unmatched files and the 93 `NOASSERTION` packages describe the output
  of [collect_qt_license_evidence.mjs](../scripts/collect_qt_license_evidence.mjs),
  a manual evidence tool that no build or workflow invokes. None of it is a
  licence obligation; it is the residue of the review apparatus removed on
  2026-09-19.
- **Fonts.** The generator copies the `epaint_default_fonts` texts
  (Hack, OFL, UFL, emoji) when that crate is in the graph.
- **Opus model data.** The DNN weights come from upstream Opus as C source
  arrays and are covered by the packaged `opus-COPYING.txt`.
- **Game and portal dependencies.** `games/` and `web-sdk/` are first-party;
  nothing third-party is vendored there.

### Ownership

Whoever writes a supplement owns its component terms and source findings; QA
owns final artifact and device acceptance. Changes to Qt, Gradle/NDK, assets,
model data, patches, or packaging require refreshed evidence. No tool verifies
that the component set is complete or that a conclusion about a licence is
correct.

## Packaged Locations

| Distribution | Notices |
|---|---|
| Windows portable archive | `licenses/client/`, `licenses/installer/`, `LICENSE.txt` |
| Standalone Windows installer | Accompanying `doubleslash-installer-win64-licenses.html` release asset |
| Linux AppImage | `usr/share/doubleslash/licenses/` inside the AppDir |
| macOS application | `Contents/Resources/licenses/` |
| Supernode archive | `licenses/` |
| Android APK/AAB | `assets/licenses/<target>/` plus preserved/merged upstream META-INF notices |

Keep the installer companion notice with standalone redistribution. Launcher
self-copy/update now requires and retains that notice, including the embedded
font notices. SignPath restores it from the original artifact and verifies the
signed archive; signed and unsigned releases retain the same companion filename
when the nightly executable is renamed.

The supernode manager retains all `licenses/` entries as `<binary>.licenses/`
and uploads them before promoting the executable. Downloaded packages and local
installs without distribution notices fail. `build-deploy` generates adjacent
notices automatically and requires Node and the pinned cargo-about version.
Manually copied local binaries need the same adjacent directory.

Android exposes packaged notices, including nested supplement notices, at
**Settings > Legal > Third-party licenses**. The reader works offline and opens
HTTPS source links through the external browser. It has no JavaScript bridge.
Device acceptance still requires a connected device.

## Release Verification

Generate notices using the same target and feature set as the binaries.
Inspect the final archives/APKs, not just staging directories, and verify
notice and source accessibility. Check included assets, runtime
DLLs/frameworks/plugins, and any separately distributed executable against the
inventory.

`node scripts/licenses/verify_artifact.mjs ARCHIVE PRODUCT [PRODUCT...]` extracts
the final `.7z`, `.zip`, `.tar.gz`, APK/AAB, AppImage, or mounted read-only DMG.
Packaging scripts invoke it after creating the archive; the signing workflow
invokes it again after SignPath. It rejects missing or empty notices, native
notices the inventory promises but the package lost, and Android ABIs lacking
inventories. This checks local readability and source-link structure, not the
availability or legal sufficiency of every remote source URL.

Packages carry notices, not sources. Nothing in any product's Rust graph
obliges source delivery: the permissive licences are notice-only, the single
MPL-2.0 crate is unmodified from crates.io and links to its exact versioned
download, and first-party and patched crates link to the public repository at
the revision recorded in `inventory.json`. Qt and externally supplied runtime
sources still need their own reviewed source information.

### Merge protection

Require the `License checks required` status on `develop` and other release
branches after the Dependency Licenses workflow has run. This aggregate fails
when any policy or notice job fails or is skipped. Repository rulesets and branch
protection must be configured in GitHub; the workflow cannot enforce them alone.
On 2026-09-16 the public API returned no repository rulesets; no authenticated
credential was available to inspect classic protection, change settings, or
dispatch Actions.

Rust policy checks and generated notices do not replace artifact review, patent
review where needed, or legal advice. What is still open is recorded under
[Known Gaps](#known-gaps), not in the backlog: licensing is no longer tracked
as a release-blocking phase.
