# Windows Qt evidence collected 2026-09-16

Collector/reviewer: Codex, technical provenance review only. This directory is
**not a distribution supplement** and deliberately contains no `review.json`.
The input was the existing `dist/DoubleSlash` staging tree and
`C:/Qt/6.8.3/msvc2022_64`, not a newly built final release archive.

`evidence.json` retains the SHA-256 of all 110 native files, the matched SPDX
records, declared license alternatives, source references, and unresolved
package metadata. `sbom/` retains seven upstream SBOM documents compressed
without changing their contents; `index.json` records their uncompressed hashes.

All 98 matched Qt binaries reproduce the upstream SBOM's pre-signing image
checksum. The comparison removes the trailing Authenticode certificate, zeroes
the certificate directory and PE checksum, and hashes the resulting image.
It does not modify shipped files or assert certificate trust. The earlier
checksum discrepancy is explained by signing for these 98 files.

The remaining 12 files include the two first-party executables and ten runtime
files without an SBOM file record:

| Component | Observed version/evidence | Remaining acceptance |
|---|---|---|
| QtWebEngineProcess | PE version 6.8.3.0 | Match helper provenance and generated Chromium notices/source to this build |
| FFmpeg DLLs | Embedded `FFmpeg version 7.1`; avcodec 61.19.100, avformat 61.7.100, avutil 59.39.100, swresample 5.3.100, swscale 8.3.100 | Retain exact Qt build recipe/patches and matching source; verify LGPL replacement procedure |
| D3Dcompiler_47 | 6.3.9600.16384 | Identify the SDK redistribution grant and applicable notices |
| dxcompiler | 1.8.0.4973 | Match upstream build/source and notices, including its embedded dependencies |
| opengl32sw | No PE version string | Identify the exact Mesa/LLVM build and its licenses/source |
| VC redistributable | 14.38.33130.0 | Retain the applicable Microsoft redistribution terms |

The FFmpeg binary contains the license string `LGPL version 2.1 or later` and
this configuration string:

```text
--prefix=installed --disable-programs --disable-doc --disable-debug --enable-network --disable-lzma --enable-pic --disable-vulkan --disable-v4l2-m2m --disable-decoder=truemotion1 --toolchain=msvc --enable-shared --disable-static
```

These strings establish useful build evidence, not proof that an unmodified
upstream tarball reproduces the deployed DLLs. FFmpeg's
[distribution guidance](https://ffmpeg.org/legal.html) requires attention to
the exact source and build configuration. Qt's
[LGPL obligations](https://www.qt.io/development/open-source-lgpl-obligations)
also require more than dynamic linking. No replacement/relinking acceptance or
codec patent clearance has been recorded here.
