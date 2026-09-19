# Backlog

Open work and acceptance criteria. Durable *shipped* invariants live in
[`agents.md`](agents.md); when an item here lands, move its invariant there and
**delete the item** rather than marking it shipped. This file is not a changelog.

Phases are **priority order**, not calendar dates. Finish or explicitly defer a
phase before treating a later one as current work. Public “in progress” claims
in [`README.md`](README.md), [`ConquerD_www/index.html`](ConquerD_www/index.html),
and [`ConquerD_www/architecture.html`](ConquerD_www/architecture.html) must have
a home here.

| Public claim | Phase |
|---|---|
| Dependency license / distribution blockers | 0 |
| Linux/macOS camera hardware validation | 1 |
| Two-client / room media acceptance | 1 |
| Incoming video (and shared-audio playout) on Android | 2 |
| Device pairing and installed simultaneous-use | 2, 6 |
| Screen and shared-audio capture outside Windows | 3 |
| WASM plugin sandbox | 7 |
| Space Layer 2 cryptography | 7 |

---

<a id="dependency-license-release-acceptance"></a>

## Phase 0 — Release licensing

On 2026-09-19 the review apparatus was removed: the build-input hashes, the
Android runtime inventory, `review.json` and the gate that refused to package
without one. None of it was a licence obligation, and it cost more than it
caught. What remains is notices - generated from the resolved graph, copied
from `packaging/licenses/<target>/<product>/`, and checked after packaging to
confirm they actually shipped. Nothing fails closed any more. Details:
[licensing guide](docs/LICENSING.md#remaining-release-approval-findings).

Implemented and locally checked on 2026-09-16 (not acceptance):

- Explained all 98 matched Qt SBOM checksum differences by reconstructing
  pre-Authenticode PE images; retained evidence in
  [the evidence directory](packaging/licenses/evidence/windows-qt-6.8.3/README.md).
- Added final-archive notice validation: an archive that lost a notice between
  staging and packaging fails. The packaged working-tree source snapshot was
  dropped on 2026-09-19 - nothing in any product's graph obliges source
  delivery, and it was 48 MB of a 140 MB APK.
- Fixed launcher self-copy and supernode-manager extraction/upload to retain
  notices. Fixed SignPath's artifact ID input and restore the standalone notice
  from the unsigned artifact before signed publication.
- Added the Android offline notice reader under Settings > Legal. Device
  acceptance remains open.
- Windows supernode release ZIP notices/source/inventory passed the archive
  validator. That does not accept the other distribution targets.

### Still blocking

1. **Runtime notices.** Desktop Settings > About has no license, About Qt, or
   Chromium credits UI. Archive `licenses/` files are not a prominent in-app
   notice. Qt LGPL requires the license text plus a prominent notice that an
   LGPL library is used. Android's Legal reader exists but has not been accepted
   on a device.
2. **Chromium and corresponding source.** The Qt WebEngine 6.8.3 SBOM lists
   wrapper DLLs only; it does not list Chromium, V8, Skia, Blink, FFmpeg, or
   `QtWebEngineProcess.exe`. Matching `Qt6WebEngineCore.dll` by reconstructed
   pre-signing checksum is not Chromium notice or source evidence. LGPL
   replacement/relinking instructions are unrecorded.
3. **Checksum and provenance.** All 98 Qt SBOM matches are reconstructed
   pre-signing PE hashes, not raw shipped-file checksums. Twelve files including
   QtWebEngineProcess and FFmpeg remain unmatched. 93 SBOM packages remain
   `NOASSERTION`. The collector inventories DLL/EXE only, so embedded components
   are outside that provenance.
4. **Supplement coverage.** The Windows supplement covers Qt, Chromium/WebEngine,
   FFmpeg, Mesa and the MSVC redistributables. Still unchecked: Android
   JVM/Oboe/NDK, fonts, game/portal dependencies, and Opus model data. Nothing
   enforces this - adding a bundled component without adding its notice is now
   a silent omission. The owner confirmed original project artwork; remaining
   review is dependency-supplied assets, not that artwork.
5. **Final artifacts.** Build and inspect Windows archives, macOS
   applications/DMGs, Linux AppImages, supernode archives, and Android APKs/AABs.
   Staging-directory checks alone are not acceptance.
6. **Publication paths.** Verify stable, nightly, signed, and unsigned paths
   retain the standalone-installer companion notice, including SignPath. Exercise
   installer-update and supernode deployment on actual installations.
7. **Android device acceptance** of the Legal reader, including supplement
   notices and external source links.
8. **GitHub enforcement.** Run the license and release workflows in GitHub
   Actions, including scheduled advisory/ban/source checks. Configure branch
   protection to require `License checks required`. Workflow wiring alone does
   not enforce merges. No authenticated GitHub credential was available when
   this was recorded; the license workflow was not yet published.
9. **Ownership.** Someone has to re-check source and replacement arrangements
   when Qt, Gradle/NDK, assets, model data, or packaging change. No tool will
   raise it.

---

## Phase 1 — Prove what already ships

Fixes and live acceptance of paths the product already advertises. Do not start
new capture backends or pairing protocols until these have a written result.

### Group-key: remaining live gaps

Election padding, catch-up minting, forward epoch jumps (`MAX_EPOCH_ADVANCE`),
and reseal-to-lagging-member landed 2026-09-12 with unit and manager-harness
tests. **Not yet verified live.**

Still open:

- A stranded member that only *listens* sends no frames, so the keyer cannot
  see it is behind; it recovers at the next membership change or rejoin.
- Unexplained: a desktop never received epochs 2 and 3, which suggests the
  keyer's membership union briefly excluded it. Capture the **keyer's** log
  next time; the phone's logcat had already rolled past the event.

**Testing note:** the public `default` room is a bad place to test room E2E. It
carries members on clients you do not control, and a single un-upgraded
participant acting as a competing keyer is indistinguishable from a local bug.
Use a private two-party room.

### Android: private-room voice and background

Direct 1:1 voice is proven. Distinct-identity desktop/phone chat passed
2026-09-13. Remaining:

1. **Room voice in a private two-party room.** The public `default` room never
   produced audible audio because a third un-upgraded participant fought for
   keyer. Retest after the group-key live capture above before drawing any
   conclusion about the room path itself.
2. **Survive the background.** Nothing has been tested across doze, screen-off,
   or a Wi-Fi/cellular handover — the three things a desktop never exercises.

### Windows two-client + room media checklist

Gate “video calls ship” on a written manual checklist, not compile. Cover:

- Direct 1:1 camera on both legs with decode to `QVideoSink`.
- Room multi-party fan-out (relay path, mid-join keyframe recovery, camera-off
  placeholders).
- Direct-call → temporary SFU fallback still routing video (`video_route`).
- Failure modes: no camera, camera in use, encoder unavailable, quota shed,
  stall vs intentional camera-off (`SfuVideoState`).

### A/V sync on a real network

Unit coverage uses a fake clock. Still unrun: clap+flash under a clean network,
~1–2 % loss, and a keyframe burst, plus a multi-peer check that per-sender
timelines never cross. Target is ±40–80 ms audio-led. Confirm the voice path is
byte-identical to before the media layer landed.

### Linux / macOS camera hardware

Camera backends compile. Nothing outside Windows has delivered a captured frame
into a call.

| Backend | Compiles | Logic tested | Run against a camera |
|---|---|---|---|
| Windows `MfCamera` | yes | yes | yes (dev machine) |
| Linux `V4l2Camera` | yes (WSL + CI) | format choice | opens + negotiates, never streams |
| macOS `AvfCamera` | Rust half via `lint-macos` | — | no Mac on the team |

Linux 2026-09-03 against a Logitech C270 in WSL: enumeration and format
negotiation work (`YUYV` chosen, 640x360). Frame delivery cannot be answered
from WSL — UVC isochronous endpoints are unimplemented on USB/IP, so
`next_frame` blocks forever. Need a real Linux machine, a VM with true USB
passthrough, or a WSL kernel with `CONFIG_VIDEO_VIVID=m`. Also still open:
stride across YU12/NV12/YUYV, buffer starvation, unplug-mid-call.

macOS still needs the Objective-C shim compiled on a Mac (`test-macos` CI is
the real signal), the TCC camera prompt, and confirmation that
`AVCaptureSessionPreset` yields the requested size. Add a macOS sibling of the
`#[ignore]`d `captures_a_frame_from_the_default_camera` test.

### Portal over real QUIC

In-tree Qt fixtures do not establish real-device QUIC acceptance. Open the same
app/room on two devices, exercise catch-up, coordinator leave, and Focus/Session
layout as described in [`games/README.md`](games/README.md). Focus Timer still
assumes synchronized device clocks; that is a demo limit, not a framework bug.

---

## Phase 2 — Advertised client gaps

Items the website lists as in progress, and the device-routing preview that
README/architecture already disclose as unfinished.

### Android incoming video and shared audio

Sending works (CameraX → `nativeSubmitCameraFrame` → VP8). Still missing:

1. **Video render.** Decoded I420 to a `Surface` via `ANativeWindow` from Rust.
   A phone can be seen but cannot see.
2. **Shared-audio playout.** Android has no content-audio capture *or* playout
   anchor. Incoming `ContentAudioReceived` must not be serialized into UI JSON;
   route it like voice in `session.rs::route_media`.

### Installed simultaneous-use acceptance

`device-routing` is the default of `doubleslash-features` and advertises
`core.devices.v1`. Routing, device keys/registries, encrypted own-device
room-key handoff, and portable backups exist. This is **not** completed pairing
or continuous sync.

Still required before claiming the preview:

- Installed desktop (normal packaged profile, not `.clientA`) + phone, same
  identity, both unlocked: both stay connected; room chat appears on both with
  own messages marked sent; an incoming call can be answered on either and
  stops ringing on the other; reconnecting one leaves the other connected.
  Repeat with a third contact and with one device on relay.
- Desktop → real Android → desktop restore with a large attachment, a missing
  attachment, a keyfile-protected source, a failed/cancelled document-picker
  copy, and a restart after profile selection. Check actual history and files.

File-based device moves still require quitting the source before connecting the
destination. Do not enable a “ready” product gate on room-chat tests alone.

Pairing UX, history sync, and delegated authorization are Phase 6.

---

## Phase 3 — Capture outside Windows

The media layer (clock, PTS, content-audio wire, audio-led sync, ABR) is
shipped. What is left is platform capture. Adding a backend is a
[`PRIVACY.md`](PRIVACY.md) change in the same commit.

### Screen / window

| Platform | Camera | Screen / window |
|---|---|---|
| Windows | `MfCamera` | `Windows.Graphics.Capture` |
| Linux | `V4l2Camera` | **open** — PipeWire + `xdg-desktop-portal` (Wayland), X11 fallback |
| macOS | `AvfCamera` | **open** — ScreenCaptureKit |
| Android | CameraX send | **open** — `MediaProjection` → `VirtualDisplay` → `ImageReader` |

Linux: Wayland has no screen-scraping API; capture goes through
`xdg-desktop-portal` ScreenCast over D-Bus and returns a PipeWire node after
the *compositor* draws the picker. That dialog is not skippable and cannot be
replaced by our QML source list — `SourceSpec::Screen` needs a portal-shaped
variant. An X11 fallback (XComposite/XShm) can keep the current id model.
macOS needs Screen Recording entitlement and usage strings; not testable in CI.
VP8 already covers encode; a backend only has to produce tightly-packed I420
`RawFrame`.

### Content-audio capture

| Platform | Source | State |
|---|---|---|
| Windows | WASAPI loopback and per-app `VAD\Process_Loopback` | Built |
| Linux | PipeWire / PulseAudio monitor source | Unbuilt |
| macOS | Virtual device or ScreenCaptureKit audio | Unbuilt |

Whichever backend a platform gets must report **device capture offsets** per
frame (`CaptureTimeline`), not frame counts: a quiet loopback emits nothing,
and a counter converts silence into permanent audio-behind-video lag.

Pairs with screen capture (same permissions on Linux/macOS).

**Echo hazard (every platform):** if content is system loopback, remote peers'
audio played locally re-enters the loop. Needs exclude-our-own-output, ducking,
or a virtual cable.

Still unsolved by design: **lip sync for a talking-head call.** The mic is not
on the synced timeline. Accepted for v1; a follow-on could additionally carry
the mic on the media layer during video calls.

---

## Phase 4 — Android distribution

App-side policy work for a first listing is in the tree (`targetSdk` 36,
`specialUse` FGS, terms gate, disclosures, Report, locked portal WebView,
incoming-call notifications). What is left is mostly Play Console, CI, and
release hardening.

1. **CI.** No Android job exists. It needs the NDK, `cargo-ndk`, and
   `cmake;3.31.6` (CMake 4 rejects libopus's `cmake_minimum_required`).
2. **Release APK hardening.** R8 rules exist (`app/proguard-rules.pro`) but a
   minified release build has never been run. `bundleRelease` signs from
   `DOUBLESLASH_KEYSTORE` when set; a Play upload key has not been used.
3. <a id="access-local-network"></a>**`ACCESS_LOCAL_NETWORK` before `targetSdk` 37.**
   At 36 the grant is implicit from `INTERNET`. Declaring the permission early
   *revokes* that grant. Raising to 37 without the request and a rationale
   turns every direct LAN session into a relayed one with no error. Do not
   declare it at 36.
4. **Call UI polish.** No speaking indicators (the core does not surface
   per-peer audio activity to this layer), no call duration, no reconnect
   affordance when a call drops.
5. **Play Console (blocks a public listing, not more Kotlin):**
   - Signed AAB + Play App Signing. Package id `com.doubleslash.client` is
     frozen at first upload.
   - `specialUse` declaration + a short video of the persistent connected
     notification and Disconnect.
   - Full-screen intent declaration (`USE_FULL_SCREEN_INTENT`).
   - Data Safety matching `PRIVACY.md`; photo/video permissions declaration;
     encryption besides HTTPS; IARC (typically Teen / 13+; not Designed for
     Families); closed testing; developer identity verification.
   - Reviewer notes: unlock passphrase + a working invite.
   - First scan you cannot fake: upload the AAB to an **internal testing**
     track and read Play Protect + the pre-launch report.

Public policy URLs (only after these files are on `develop`):

- https://github.com/DoubleSlashSpace/DoubleSlash/blob/develop/PRIVACY.md
- https://github.com/DoubleSlashSpace/DoubleSlash/blob/develop/TERMS.md

**Not doing for Play:** `RECEIVE_BOOT_COMPLETED` autostart; in-app APK updater;
a second HTML privacy page on `doubleslash.space`.

Optional later: full Telecom `ConnectionService` / `phoneCall` FGS; a dedicated
privacy email (GitHub issues is what `PRIVACY.md` lists today).

Android already has: CameraX send, portal WebView, SAF file staging, Keystore
stay-unlocked, Legal reader, backups. Do not re-open those as missing.

---

## Phase 5 — Media and desktop polish

After the Windows checklist (Phase 1) and any new capture (Phase 3):

1. **Receiver resilience.** PLI/FIR cadence, idle decoder GC under many room
   members, graceful degrade when Qt Multimedia / sink is absent. Align
   drop/keyframe policy with the shipped A/V sync hold queue so a PLI storm
   does not empty the video timeline while audio keeps playing.
2. **Video UX.** Screen-share picker may still be thin; multi-tile layout under
   several active room cameras; confirm local-preview vs remote tile identity;
   separate level meters / mute for voice vs content when both are live.
3. **Polyphase resampling (V7).** Linear interpolation is near-inaudible for
   8 kHz voice at typical device rates; a windowed-sinc or `rubato`
   `FastFixedIn` drop-in if it becomes perceptible.
4. **Stereo / spatial mixdown (V8).** Per-peer pan in `mix_pcm_frames` + stereo
   ring buffer. Low-priority UX.
5. **Ollama / plugin UX polish** — currently experimental on desktop. Android
   has no plugin or Ollama surface; that is not a Phase 5 requirement.
6. **Android room per-peer grants** (`generateRoomInviteForPeer`) are not wired;
   shareable links carry a Space inclusion proof and no grant.

### Video non-goals (near-term)

- Multiple independent outbound streams per peer (camera + screen without
  composite) — wire identifies a stream by sender only.
- Frame-perfect / broadcast A/V sync — v1 target is ±40–80 ms audio-led.
- WebRTC / browser video — in-app portal stays on identity QUIC.
- In-tree AVC (openh264) — rejected for MPEG-LA exposure; Windows uses OS MFTs.
- Simulcast / SVC — single encode ladder + ABR first.
- WebSocket fallback for room video — relay-datagram-only; WS-only members keep
  audio, not video.

---

## Phase 6 — Devices as a product

Phase 2 is acceptance of the routing preview. This phase is the remaining
product:

1. **Short-lived QR pairing**, approval on both devices, and a dedicated
   authenticated capability with explicit device authorization and quotas.
   Supernodes forward opaque bytes only. Old clients must reject unsupported
   linking visibly; copying a seed cannot be the fallback.
2. **Delegated live authorization.** `DeviceTrustStore` persists encrypted
   registries with predecessor/revocation/fork checks, but is not consumed by
   live authentication. Current gated device routes require full root identity
   authority. `open_with_key` separates data access from signing at the API
   boundary; it does not distribute storage keys to another device.
3. **Idempotent history sync** with stable message IDs, deletion/block
   tombstones, resumable attachment transfer, and deterministic offline
   conflict resolution. Without a history-holding backend, catch-up requires
   another reachable device holding the missing data.
4. **Remaining endpoint-state integration:** device-scoped file / room-media /
   game state, and coordinated cluster device rosters.
5. **Explicit device removal** and tests for offline catch-up and revoked
   devices.

See [Devices and backups](docs/DEVICES_AND_BACKUPS.md).

---

## Phase 7 — Deferred architecture

Planned, not current. Website/README mention WASM as planned; architecture.html
lists Space Layer 2 and WASM as deferred. Keep them here so those claims stay
honest.

### WASM plugin sandbox

Bespoke feature modules are native cdylibs with load-time trust prompts. A WASM
sandbox would remove the “trust the binary” requirement. Native plugins run
in-process today; registry replacement is not hot reload and is not a sandbox.

### Space Merkle tree — remaining

Layer 1 (authenticated room tree, admission proofs, reserved Layer 2 leaf
fields, `v1` hash-label freeze) is shipped. Invariants are in `agents.md`.

- **Root-equivocation: append-only history tree.** We use a **set** tree, not
  an append-only log, so there are no consistency proofs between epochs. A
  malicious owner can sign two different roots for the same epoch. Today's
  equivalent (creator controls the room set on the supernode it talks to) is
  strictly weaker, so this is not a regression; equivocation detection +
  logging is shipped. Still deferred: CT-style append-only history with
  consistency proofs — which could also make the cluster roster itself an
  auditable membership log.
- **`Closet` node kind.** Distinct node `kind` for a different semantic
  (e.g. text-only sub-channels). Additive; not required for nesting depth.
- **Layer 2 — key hierarchy (entirely unbuilt).** Extends pairwise sender-keys
  `GroupKeySource` (TreeKEM declined). Scope: per-node epoch secrets; HKDF
  inheritance down `inherit=true` edges; `inherit=false` compartments as their
  own pairwise-distributed group; real values for reserved `inherit` /
  `key_commit` / invite `space_node_key`; capability-token admission under the
  node key without changing the admission call shape. Confirm before freezing
  `v1` hash labels: a `SpaceGrant` targets exactly one `node_id` and proofs
  don't inherit.

Space non-goals: literal MTC / X.509; subtree delegation in v1 (owner-signed
delegation leaves are a compatible later addition); inter-cluster federation,
blinded tokens, payments.

### Post-quantum crypto (ML-KEM / ML-DSA) — assessed 2026-07-11

Codebase is fully classical (Ed25519 + X25519 + AES-256-GCM/HKDF-SHA256).
Symmetric bulk AEAD is PQ-adequate *if keys are*; quantum risk is key agreement
(harvest-now-decrypt-later) and signatures (forge after CRQC). Defaults if/when
built: **ML-KEM-768** + **ML-DSA-65**, hybrid with classical.

| Role | Classical today | PQ approach |
|---|---|---|
| Invite session key | Ephemeral X25519 → HKDF (`doubleslash-invite-session-v1`) | Hybrid: X25519 ss ‖ ML-KEM ss → new HKDF info (`…-v2-hybrid`) |
| Pairwise relay / `SfuGroupKey` wrap | Static Ed25519→Montgomery DH (no FS) | KEM-DEM (finding 2); prefer ephemeral hybrid where interactive |
| Room content | AES-GCM under sender keys | Unchanged AEAD; only key *wrap* migrates |
| Identity / signaling / invites / Space roots | Ed25519 | Dual-sign transition; high-rate envelopes stay Ed25519 early |
| Release + module manifests | Offline Ed25519 | Cheap early dual-sign (long-lived, low rate) |
| QUIC/WS TLS | rustls + `ring` | Hybrid group via `aws-lc-rs` provider swap (finding 1) |
| Browser `web-sdk` | `@noble/ed25519` | After native wire freeze (JS/WASM PQ) |

Findings, still in this order if built:

1. **Hybrid PQ TLS (transport only).** Switch rustls 0.23 from the pinned `ring`
   provider to `aws-lc-rs` for `X25519MLKEM768`. Hard-coded
   `rustls::crypto::ring::default_provider()` call sites (`quic_tls.rs`,
   `relay.rs`, `main.rs`, cluster-link tests), larger/slower builds, possible
   dual-provider graph, CI matrix risk. Protects only transport TLS HNDL
   between upgraded peers; invite X25519, pairwise relay keys, and room AES
   keys stay classical. Do not market as “PQ-ready.”
2. **`derive_pairwise_relay_key` cannot be ported.** It relies on the
   Ed25519→Montgomery birational map; ML-DSA keys have no map to ML-KEM.
   Replacement is KEM-DEM: each identity carries a second static ML-KEM key
   signed by the ML-DSA identity key. Direction now matters. Affects
   `EncryptedSignal` and `SfuGroupKey` sealing.
3. **Invite handshake maps cleanly** but the blob grows 32 B → 1,184 B of key
   material + 1,088 B reply. Re-check invite-link and QR-code size limits.
4. **Keep Ed25519-derived `public_id` as the stable peer id**; attach
   `ml_dsa_pub` as a verified binding. ML-DSA-65 pubkeys are 1,952 B.
5. **Do not dual-sign high-rate envelopes early.** Prefer invites, Space roots,
   grants, tickets. Exception: release- and module-manifest signing is cheap
   and long-lived.
6. **Identity storage:** FIPS 203/204 seed-based keygen; keep storing small
   seeds.

Suggested build order: (1) hybrid TLS provider, (2) hybrid invite handshake,
(3) KEM-DEM pairwise / group-key wrap, (4) dual-sign low-rate + manifests,
(5) identity ML-DSA binding, (6) browser parity, (7) deprecate pure classical
by policy only after ecosystem age.

PQ non-goals: pure ML-KEM without X25519 hybrid on first ship; ML-DSA on every
SFU audio frame; reviving TreeKEM for PQ rooms; QUIC-stack PQ as a hard
dependency of app-layer PQ; SLH-DSA / FN-DSA unless a later trade-off demands
it.

Layer 1 still helps a future PQ migration: admission/directory trust costs
**one signature per Space per epoch**.

### Discovery / federation (only if demand appears)

- In-band capability gossip of supernode bundles while the invite-only trust
  root stays intact.
- Signed `RelayAd` + capacity-aware selection, once relay sets grow past a
  hand-managed cluster.
- Inter-cluster federation or a DHT — only if cross-operator federation beyond
  “link into one cluster” is required.

---

## Declined / out of scope

Matches the website “Not planned” column plus earlier design rejections.

- **Accounts, a user directory, or discovery.** Identity and trust stay
  client-owned and invite-only.
- **A supernode that can read content.** Nodes route opaque bytes.
- **A public web client / WebTransport game listener.** Portal pages run
  inside the native app.
- **Telemetry or analytics.**
- **Custom minimal TreeKEM.** Rooms are invite-only and sized for manual
  invitation; O(N) pairwise rekeying is not the bottleneck. Revisit only if
  room size or churn actually bites.
- **Per-message forward secrecy (double-ratchet).** Marginal for ephemeral SFU
  rooms rekeyed on membership change.
- **Payments / usage receipts.** Conflicts with the no-backend,
  volunteer-supernode model.
- **Read receipts (`MessageStatus::Read`).** If ever revisited, gate behind a
  privacy toggle defaulting **off**.
- **Room-chat history for joiners / offline store-and-forward.** The supernode
  does not persist messages.
- **Durable first-party portal documents.** Demo state lives in open pages;
  closing the last page loses it. Collaborative-tool durability is an app
  problem, not a relay cache.
