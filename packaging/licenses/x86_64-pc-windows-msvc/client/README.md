# Windows client license supplement

This directory holds the notices required to redistribute the Windows bundle:
everything in it that is **not** a Rust crate. Rust dependencies are handled
automatically and end up in `licenses/client/rust-licenses.html`.

`build_win64.ps1` and the release workflow find this via
`DOUBLESLASH_LICENSE_SUPPLEMENT`. Its contents are copied into
`licenses/client/supplement/` in the shipped bundle and surfaced in the app
under Settings > About > Third-Party Notices.

## What is here

| Component | Licence | Notices |
|---|---|---|
| Qt 6.8.3 | LGPL-3.0 | `qt-6.8.3-LGPL-3.0.txt`, `qt-6.8.3-source-information.txt` |
| Qt WebEngine 6.8.3 + Chromium 122 | LGPL-3.0 + BSD-3-Clause | `chromium-122-LICENSE.txt`, `chromium-122-source-information.txt` |
| FFmpeg 7.1 | LGPL-2.1-or-later | `ffmpeg-7.1-LGPL-2.1.txt`, `ffmpeg-7.1-source-information.txt` |
| Mesa llvmpipe | MIT | `mesa-license.txt`, `mesa-source-information.txt` |
| Microsoft DirectX compilers | MS redistributable terms | `microsoft-redistributables-source-information.txt` |
| Microsoft VC++ Runtime | MS redistributable terms | `microsoft-redistributables-source-information.txt` |

`relinking-instructions.txt` covers the LGPL substitution right for both Qt and
FFmpeg.

## The position in one paragraph

Every third-party native component ships **unmodified**, from official upstream
binaries, and is **dynamically linked** as a separate library. That is the
comfortable case for LGPL: there are no modifications to disclose, and the
relinking right is satisfied by file substitution rather than by shipping
object files. What remains is notice and source-availability, which the files
above provide.

## Keeping it accurate

The only thing that makes this stale is **changing what ships**. After a Qt
upgrade, a new bundled library, or a feature change that pulls in something
new, update the affected `*-source-information.txt` and the matching
`components` entry in `review.json`.

`features` in `review.json` must match the build. It is checked because it
changes the component set: without the `webengine` feature there is no Chromium
in the bundle, and with it there is.

## Open items

Recorded in the relevant `*-source-information.txt` rather than papered over:

1. **Chromium third-party credits.** `chromium-122-LICENSE.txt` is Chromium's
   own BSD-3-Clause licence, not the generated credits set for the components
   bundled inside the snapshot. Producing it needs a 122-based
   `qtwebengine-chromium` checkout and Chromium's `tools/licenses/licenses.py`.
2. **Microsoft redistribution rights.** Confirm the project's Visual Studio /
   Windows SDK licence permits redistributing `D3Dcompiler_47.dll`,
   `dxcompiler.dll` and `vc_redist.x64.exe` this way — or drop `vc_redist` and
   document the runtime as a prerequisite.
3. **Mesa version.** Not recorded by Qt in the deployed artifact.
4. **Opus model archive.** Flagged in
   `../../evidence/embedded-components/README.md`: the generated Opus model
   arrays come from a separately downloaded hash-named archive whose
   redistribution grant has not been confirmed. Outside this supplement (Opus
   is handled on the Rust/native notice path) but it is the one remaining
   licence question that is not about Qt.

Items 1 and 4 are the two worth closing before a public release. The rest are
recordkeeping.
