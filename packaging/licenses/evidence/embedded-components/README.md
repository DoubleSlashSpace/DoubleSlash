# Embedded component evidence

Collected/reviewed by Codex on 2026-09-16 as a technical inventory, not a
distribution or patent approval.

- `fonts.json` hashes the exact four fonts and accompanying notices in
  `epaint_default_fonts` 0.31.1. The notice generator includes Hack's full
  notice, the emoji icon font MIT notice, OFL, and Ubuntu Font Licence text in
  the installer's HTML, so standalone redistribution retains them. Source is
  the exact versioned crate archive recorded in the inventory.
- `notice-sources.json` identifies the source and hash of Qt 6.8.3 LGPL/GPL
  and FFmpeg 7.1 LGPL texts. These texts alone do not establish which upstream
  source/configuration produced the deployed binaries.
- Project-owned artwork, icons, and sounds were confirmed by the owner. The
  currently embedded supernode portal/game templates import the project's
  own relative modules; no npm lockfile or external script URL was found in
  that template set. Operator-added content requires its own review.
- Opus is pinned at `f18e26e648e0b7d0ad3db5b98eca461c937ee757`; libvpx at
  `ade52487a37ef76a0f209bd39bea9fe67d6db4c4`. Their COPYING/LICENSE/PATENTS
  notices are packaged. The generated source archive retains modified local
  sources and the actual generated Opus C model arrays, not just these revisions.
- Opus model data comes from the separately downloaded, hash-named archive
  `a5177ec6fb7d15058e99e57029746100121f68e4890b1467d4094aa336b6013e`.
  Generated array headers identify checkpoints but do not themselves contain
  a redistribution grant. Confirm the grant covering that exact model archive
  and its applicable patent terms; the general
  [Opus license](https://opus-codec.org/license/) must not be treated as a
  finding about every separately supplied model.

Resolved Android POMs, JAR/AAR notices (including nested JARs), NDK notices,
and per-ABI C++ runtime hashes are collected by the Gradle
`collectDebugRuntimeLicenses` and `collectReleaseRuntimeLicenses` tasks.
The resolved evidence and Oboe/build-script embedded dependencies still need
component review before an Android supplement can be approved.
