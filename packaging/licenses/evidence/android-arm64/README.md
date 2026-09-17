# Android arm64 runtime evidence

Collected by Codex on 2026-09-16 with Java 17 and the project's Gradle 8.11.1
wrapper. `compileDebugKotlin`, `collectDebugRuntimeLicenses`, and
`collectReleaseRuntimeLicenses` completed successfully. No device was connected.
This is technical evidence, not an approved Android distribution supplement.

| Variant | Resolved JVM artifacts | Embedded notices found | NDK |
|---|---:|---:|---|
| debug | 70 | 3 | 28.2.13676358 |
| release | 67 | 3 | 28.2.13676358 |

`runtime-debug.json` and `runtime-release.json` record artifact/POM hashes,
embedded notice paths, NDK notice hashes, and arm64 hashes for `libc++_shared.so`,
`libc++_static.a`, and `libc++abi.a`. The C++ entries inventory available runtime
inputs; they do not assert that all three are packaged or linked.

The resolved Android Cargo feature tree uses `oboe-sys` 0.6.1 through the local
cpal patch. Its embedded `oboe/include/oboe/Version.h` identifies Oboe 1.8.1 and
contains an Android Open Source Project Apache-2.0 header. `fetch-prebuilt` and
`shared-stdcxx` are absent from that resolved feature tree; its build script
compiles the packaged C++ sources and selects `c++_static`. Matching sources are
in the [oboe-sys 0.6.1 crate archive](https://crates.io/api/v1/crates/oboe-sys/0.6.1/download).
The Rust notices contain Apache-2.0; the NDK notices collected here cover the
separate runtime review. This finding does not cover a future feature change
that downloads a prebuilt native library.

`notices-and-poms.tar.gz` preserves the collected POMs and actual JVM/NDK notices
under `debug/` and `release/`. An artifact without an embedded notice is not
automatically unlicensed or exempt: use its POM, matching source archive and
upstream notices to complete the component review. There are 67 such debug
artifacts and 64 release artifacts. The collector does not invent their notices.

Before distribution, finish the Oboe/native build-script and model-data reviews,
prepare target/variant supplements, inspect APK/AAB contents, and exercise the
Settings > Legal > Third-party licenses reader on a device. Repeat collection
for any additional shipped ABI or changed Gradle/NDK resolution.
