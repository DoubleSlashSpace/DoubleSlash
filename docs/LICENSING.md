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

For an audit without approving distribution:

```sh
node scripts/generate_licenses.mjs --product client --target x86_64-pc-windows-msvc --features qt-ui,webengine --rust-only --output dist/license-audit/client
```

Generated directories contain readable Rust license texts, the project license,
an inventory of crate versions, the target/features, and the Cargo lockfile
SHA-256. Client and Android directories also contain the vendored Opus and
libvpx license and patent notices. Source links identify exact crates.io
archives or the bundled working-tree source snapshot. That snapshot preserves
modified local dependencies; its hash is recorded separately from the Git
revision. Verify source accessibility when distributing.

The policies include font licenses for the installer's embedded fonts and the
CA-root data license. Permission to bundle a font does not permit relicensing
it under MIT, selling it alone where prohibited, or ignoring reserved-name
conditions. An allowed SPDX identifier is not a determination that every
condition has been satisfied.

## Supplemental Review

Desktop and distributable Android packages require a reviewed supplement.
No approved supplements are committed yet, so those distribution builds are
intentionally blocked. Ordinary `cargo build` remains available. Local Android
debug builds generate audit-only notices; setting `DOUBLESLASH_DISTRIBUTION=1`
makes debug builds require the same review as release builds. Android CI sets it.

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
These are investigation results for that local bundle, not approval of future
builds. Chromium's embedded third-party notices and corresponding-source
arrangements remain separate requirements even when a Qt DLL is matched. The
three gaps that still block release approval are recorded under
[Remaining Release-Approval Findings](#remaining-release-approval-findings).

Each supplement directory contains the actual notice/source-information files
and a `review.json` with this structure (replace every example value):

```json
{
  "product": "client",
  "target": "x86_64-pc-windows-msvc",
  "features": "qt-ui,webengine",
  "components": [
    {
      "name": "Exact shipped component",
      "version": "Exact shipped version",
      "source": "Location of the matching source",
      "obligations": "How notices, source access, modifications, and other applicable conditions are satisfied",
      "files": ["component-license.txt", "component-source-information.txt"]
    }
  ]
}
```

The generator checks that `product`, `target` and `features` match the build,
that every component names a version, a source and how its obligations are met,
and that each notice file exists, is nonempty, and lives inside the supplement
directory. After the archive is built, `verify_artifact.mjs` re-extracts it and
confirms those notice files actually shipped and are readable.

`features` is checked because it changes what ships — a build with the
`webengine` feature bundles Chromium and needs its notice, so a supplement
written for the other feature set would silently under-notice.

Android supplements have the same shape. `runtime-inventory.json` from
`:app:collectDebugRuntimeLicenses` / `:app:collectReleaseRuntimeLicenses` (with
reports under `app/build/reports/licenses/`) remains the way to enumerate
resolved artifacts, POMs, nested JAR notices and NDK notices when writing the
component list. Collection is not review: resolve components without sufficient
license/source information before listing them.

### What this does and does not do

These checks confirm the required notices are present, describe what ships, and
reach the recipient. They are not an audit trail. There is deliberately no
reviewer signature, no lockfile or build-input binding, and no per-binary hash
inventory: none of that is a licence obligation, and requiring it blocked
packaging without making the distribution any more compliant.

No tool verifies that the component list is complete or that its conclusions
about a licence are correct. That judgement stays with whoever writes the
`obligations` text. Do not list a component you have not actually checked.

Release workflows look for desktop supplements under
`packaging/licenses/<target>/client/` and Android supplements under
`packaging/licenses/android/<target>/<variant>/`. For local desktop packaging, set
`DOUBLESLASH_LICENSE_SUPPLEMENT` to the review directory. For Android, set it
to `packaging/licenses/android` (containing target and variant subdirectories).
These files are public distribution materials, not credentials.

## Remaining Release-Approval Findings

Recorded 2026-09-16 from the retained Windows Qt 6.8.3 evidence, the Qt
WebEngine SBOM, the Android runtime collectors, and the current client
surfaces. These are technical gaps, not legal advice. Desktop and
distributable Android packaging remain blocked until a named reviewer records
how each gap is closed in a target-specific supplement.

### Runtime notices

Qt's [open-source LGPL obligations](https://www.qt.io/development/open-source-lgpl-obligations)
require a copy of the LGPL text and a prominent notice that an LGPL library is
used. Chromium's BSD license requires reproducing its copyright notice in the
documentation or other materials provided with the distribution.

Current surfaces:

- Desktop Settings > About shows the product name, version, and shortcut
  controls only. There is no in-app license list, About Qt dialog, or
  Chromium credits page.
- Packaged `licenses/` directories in archives hold Rust notices and, once
  approved, supplement files. Archive files are not a runtime prominent
  notice.
- Android Settings > Legal > Third-party licenses reads packaged notices
  offline. Device acceptance is still open, and no approved native/runtime
  supplement is packaged yet. Android uses the system WebView, so the desktop
  Qt WebEngine/Chromium notice set does not apply there; JVM, Oboe, NDK, and
  other reviewed texts still need to appear in that reader.

Do not treat the Rust HTML notice or the Android reader as covering Qt,
Chromium, FFmpeg, or NDK/JVM notices until those texts are in the reviewed
package and reachable from the corresponding UI.

### Chromium and corresponding source

The retained
[qtwebengine 6.8.3 SBOM](../packaging/licenses/evidence/windows-qt-6.8.3/sbom/)
lists 10 packages and 23 files: Qt wrapper DLLs, import libraries, and
plugins. It does not list `QtWebEngineProcess.exe`, Chromium, V8, Skia, Blink,
or FFmpeg. `Qt6WebEngineCore.dll` matched the SBOM by reconstructed
pre-signing PE checksum to
`qtwebengine.git@b586c4eb65d8e46ab2c255e1a141676043a650da`. That match
identifies the wrapper binary; it is not Chromium third-party notice evidence
and not corresponding source for the embedded snapshot.

[Qt WebEngine licensing](https://doc.qt.io/qt-6/qtwebengine-licensing.html)
states that Chromium is built into Qt WebEngine Core and that distributors
must comply with both the Qt WebEngine licenses and Chromium's licenses. The
most restrictive Chromium license called out there is LGPL 2.1. Use the
6.8.3 documentation and source, not a later snapshot, when completing this
review. FFmpeg 7.1 in the inspected bundle embeds `LGPL version 2.1 or later`
and a specific configure string; that is build evidence, not a source offer.

The first-party `corresponding-source.tar.gz` snapshot covers the working
tree, including modified local dependencies and generated Opus model arrays.
It does not contain Qt, Chromium, or FFmpeg sources. Replacement and
relinking instructions for LGPL libraries have not been recorded. Generate
Chromium third-party notices from the matching Qt WebEngine/Chromium checkout
and retain the exact source used to build the shipped `QtWebEngineProcess`
and FFmpeg DLLs.

### Checksum and provenance verification

The inspected Windows bundle had 110 DLL/EXE files. All 98 SBOM matches used
`sbom-pre-signing-pe-checksum`: the collector strips the trailing Authenticode
certificate and zeroes the certificate directory and PE checksum, then hashes
that image. None matched a raw SBOM checksum of the shipped file. That
reconstruction explains the earlier hash mismatch for those 98 files. It does
not prove certificate trust, completeness of embedded components, or identity
of the 12 unmatched files: `QtWebEngineProcess.exe`, the five FFmpeg DLLs,
`D3Dcompiler_47.dll`, `dxcompiler.dll`, `opengl32sw.dll`, `vc_redist.x64.exe`,
and the two first-party executables.

93 SBOM packages in the matched documents still have `NOASSERTION` for
license and/or source, including the `qtwebengine` root package, MSVC,
bundled zlib/pcre2, and Unicode data. A DLL checksum match does not resolve
those package entries.

The collector inventories DLL/EXE files only. `verify_artifact.mjs` can
enforce reviewed native SHA-256s on a final archive, but no approved desktop
or Android supplement exists, so that check has not been run against a
reviewed client package. Windows supernode archive validation passed because
the supernode product does not ship this Qt/Chromium runtime set.

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

Android exposes packaged notices, including nested supplemental notices, at
**Settings > Legal > Third-party licenses**. The reader works offline and opens
HTTPS source links through the external browser. It has no JavaScript bridge.
Device acceptance still requires a connected device and a reviewed build.

## Release Verification

Generate notices using the same target and feature set as the binaries. Never
package `--rust-only` audit output as approved distribution notices. Inspect
the final archives/APKs, not just staging directories, and verify notice and
source accessibility. Check included assets, runtime DLLs/frameworks/plugins,
and any separately distributed executable against the reviewed inventory.

`node scripts/licenses/verify_artifact.mjs ARCHIVE PRODUCT [PRODUCT...]` extracts
the final `.7z`, `.zip`, `.tar.gz`, APK/AAB, AppImage, or mounted read-only DMG.
Packaging scripts invoke it after creating the archive; the signing workflow
invokes it again after SignPath. It rejects audit-only/missing notices, changed
source snapshots, mismatched reviews/native files, and Android ABIs lacking
inventories. This checks local readability and source-link structure, not the
availability or legal sufficiency of every remote source URL.

`corresponding-source.tar.gz` preserves the current repository source files,
including modified dependency sources and generated model arrays; its hash is
recorded in `inventory.json`. First-party source links in bundled notices point
to that archive. Registry dependencies link to exact versioned crate downloads.
Installer and Android HTML omit local source-archive links (standalone notices
and the offline reader do not download local archives). The source snapshot
remains in each package. Qt and externally supplied runtime sources
still need their own reviewed source information.

### Ownership and merge protection

The release maintainer owns the licensing inventory and assigns a named reviewer
in each supplement. That reviewer owns component terms/source findings; QA owns
final artifact and device acceptance. Changes to Qt, Gradle/NDK, assets, model
data, patches, or packaging require refreshed evidence. Automated collection by
Codex is labeled as technical evidence, not that named reviewer's approval.

Require the `License checks required` status on `develop` and other release
branches after the Dependency Licenses workflow has run. This aggregate fails
when any policy or notice job fails or is skipped. Repository rulesets and branch
protection must be configured in GitHub; the workflow cannot enforce them alone.
On 2026-09-16 the public API returned no repository rulesets; no authenticated
credential was available to inspect classic protection, change settings, or
dispatch Actions. The new license workflow had not yet been published.

Rust policy checks and generated notices do not replace this artifact review,
patent review where needed, or legal advice. Outstanding acceptance work is
tracked in [the backlog](../backlog.md#dependency-license-release-acceptance).
