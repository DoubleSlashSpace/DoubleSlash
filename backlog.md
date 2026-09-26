# Backlog

Open work and acceptance criteria. Durable *shipped* invariants live in
[`agents.md`](agents.md); when an item here lands, move its invariant there and
**delete the item** rather than marking it shipped. This file is not a changelog.

Phases are **priority order**, not calendar dates. Finish or explicitly defer a
phase before treating a later one as current work. Public “in progress” claims
in [`README.md`](README.md) and on the website must have a home here.

| Public claim | Phase |
|---|---|
| Linux/macOS camera hardware validation | 1 |
| Two-client / room media acceptance | 1 |
| Shared-audio playout on Android | 2 |
| Device pairing and installed simultaneous-use | 2, 6 |
| Screen and shared-audio capture outside Windows | 3 |
| WASM plugin sandbox | 7 |
| Space Layer 2 cryptography | 7 |

---

## Phase 1 — Prove what already ships

Fixes and live acceptance of paths the product already advertises. Do not start
new capture backends or pairing protocols until these have a written result.

### Unified room member list and trust invites

The room member list now lives in the desktop `VoiceRail` and the Android
Members sheet: in voice / text only, with a shared per-member menu.
Room-member trust invites (`TrustRequest` sealed in `EncryptedSignal`) have
unit, bridge and two-manager tests. **Not yet verified live:**

- Desktop ↔ Android, both directions, on a private two-party room:
  1. Invite, Accept, then the peer appears in Peers on both sides, and
     Message opens the chat.
  2. Not now leaves nothing trusted.
  3. A second invite within a minute is refused with a notice.
- In-voice marks follow joins and leaves while a room is browsed without
  voice, across a cluster (the union of per-node voice rosters).
- Watch video from the member list, Stop watching for a popped-out window,
  and the level rings in the list while voice is live in that room.
- Invite handshake gate (issued-invite check, unit and manager tests only):
  1. An ordinary pasted link still completes desktop ↔ Android.
  2. The same link used a second time by someone else is refused, with a
     banner on their side.
  3. A link minted before the inviter restarts is refused afterwards.
  4. With the inviter's desktop and phone both online as one identity, a link
     minted on one still completes. The other device's refusal must not
     surface as a failure.

### Group-key: remaining live gaps

Election padding, catch-up minting, forward epoch jumps (`MAX_EPOCH_ADVANCE`),
reseal-to-lagging-member, and a listen-only member asking the keyer when it
hears an epoch it cannot open all have unit and manager-harness tests.
**Not yet verified live** — run [acceptance §1](docs/ACCEPTANCE.md#1-group-key-convergence-live).

Still open:

- Unexplained: a desktop never received epochs 2 and 3, which suggests the
  keyer's membership union briefly excluded it. The keyer now logs every union
  change with its per-node snapshot sizes (`keyer view of room … changed via`)
  and each rotation's recipients; capture the **keyer's** log next time. The
  member should now recover by itself on the first frame it cannot open, so a
  repeat shows up as a short gap plus a key request rather than lasting
  silence.

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

Both steps are scripted in [acceptance §2](docs/ACCEPTANCE.md#2-android-private-room-voice-and-the-background).

### Windows two-client + room media checklist

Gate “video calls ship” on the written manual checklist, not compile. It is
[acceptance §3](docs/ACCEPTANCE.md#3-windows-two-client--room-media); nobody has
run it yet. It covers direct 1:1 camera on both legs, room fan-out with
mid-join keyframe recovery and camera-off placeholders, the direct-call → SFU
fallback still routing video, and the failure modes (no camera, camera in use,
encoder unavailable, quota shed, stall vs intentional camera-off).

### A/V sync on a real network

Unit coverage uses a fake clock. Still unrun: clap+flash under a clean network,
~1–2 % loss, and a keyframe burst, plus a multi-peer check that per-sender
timelines never cross. Target is ±40–80 ms audio-led. Procedure:
[acceptance §4](docs/ACCEPTANCE.md#4-av-sync-on-a-real-network). If the
keyframe burst empties the video hold queue while audio keeps playing, align the
receiver's drop/keyframe-request policy with the sync hold queue.

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
passthrough, or a WSL kernel with `CONFIG_VIDEO_VIVID=m`. Padded-stride
conversion is unit-tested for all three formats. Dequeues now time out, so a
stalled driver is reported lost instead of wedging the capture thread; that
and unplug-mid-call still need a device
([acceptance §5](docs/ACCEPTANCE.md#5-linux--macos-camera-hardware)).

macOS still needs the Objective-C shim compiled on a Mac (`test-macos` CI is
the real signal), the TCC camera prompt, and confirmation that
`AVCaptureSessionPreset` yields the requested size. The `#[ignore]`d
`captures_a_frame_from_the_default_camera` test now has a macOS sibling that
prints the delivered size; it has never been compiled on a Mac.

### Portal over real QUIC

In-tree Qt fixtures do not establish real-device QUIC acceptance. Open the same
app/room on two devices, exercise catch-up, coordinator leave, and Focus/Session
layout as described in [`games/README.md`](games/README.md) and
[acceptance §6](docs/ACCEPTANCE.md#6-portal-over-real-quic). Focus Timer still
assumes synchronized device clocks; that is a demo limit, not a framework bug.

---

## Phase 2 — Advertised client gaps

Items the website lists as in progress, and the device-routing preview that
README/architecture already disclose as unfinished.

### Android incoming video and shared audio

Sending works (CameraX → `nativeSubmitCameraFrame` → VP9, or VP8). Receiving is
implemented: the voice rail's or call card's per-peer menu opts in, decoded I420
is drawn into a `TextureView` surface through `ANativeWindow`, and room
subscriptions are narrowed to the watched set. Still missing:

1. **Video receive on a device.** Host tests, clippy and an APK build pass; no
   phone has shown a picture yet. Run: room with a desktop camera on, open it
   from the rail, confirm the picture and aspect ratio, full screen, stop
   watching, camera off/on while watched, leave and rejoin (tile must not
   reappear unasked, but the sender must still show as streaming and be
   watchable again), a direct call both ways, and a desktop that chose H.264
   in Settings (expected: no picture). Check logcat for
   `[room.video.sfu] subscribing` with an empty set on voice join, and for
   `starting Android capture (vp9` when the phone's camera goes into a room.
2. **Shared-audio playout.** Android has no content-audio capture *or* playout
   anchor. Incoming `ContentAudioReceived` must not be serialized into UI JSON;
   route it like voice in `session.rs::route_media`.

### Screen share at 1080p60 and HDR: live acceptance

A 1080p60 desktop share looked like a slideshow to a viewer. The sender's log
showed Media Foundation H.264 keyframes of 330–560 KB, and every one was
dropped at the old ~130 KB fragment ceiling. The ceiling is now ~510 KB and the
encoders are capped below it. A frame that still does not fit is dropped before
it is sent and followed by a keyframe request. If keyframes keep coming out too
large, the bitrate is capped. An HDR desktop is now captured as FP16 and tone
mapped on the GPU, and every capture is scaled there. Unit tests cover all of
this, the shader included on WARP. Still to do:

1. **1080p60 live check.** Share a detailed desktop at 1080p60 in each codec
   (VP9, then H.264 from Settings). Every 10 s the sender logs a line starting
   `[video] screen <codec>: sent N/60 fps`. N should stay near 60, capture and
   encode should each fit in the 16 ms frame, and `oversized` should be 0 after
   the first keyframe. If the sender keeps its rate and the viewer still
   stutters, the problem is downstream. Look at the viewer's decoder: libvpx
   decodes on one thread, in plain C on x86.
2. **HDR live check.** On an HDR display, the capture log should read
   `capturing an HDR display (SDR white N nits), scaling on the GPU`, with N
   matching Windows' SDR content brightness. The viewer's picture should match
   the desktop's contrast. Also check a window share on the HDR display and an
   SDR display on the same machine.
3. **Viewers on older builds** still drop any keyframe over 128 fragments. Ship
   the receiver change before relying on large keyframes.

### VP9 room default: live acceptance and x86 SIMD

Rooms now send VP9, and VP8 takes over mid-call if a device cannot encode VP9
in real time. The numbers behind that decision come from
`rust/doubleslash-vpx/examples/encode_bench.rs`, run on the Xiph FourPeople
conference clip (one encoder thread):

| | VP8 | VP9 | Quality, same bitrate |
|---|---|---|---|
| Pixel 11, NEON, 360p | 1.7 ms | 1.2 ms | +0.9 dB |
| Pixel 11, NEON, 720p | 6.9 ms | 4.5 ms | +0.6 dB |
| x86 desktop, generic C, 720p | 16 ms | 28 ms (15 ms on 2 threads) | +1.5 dB |

At equal quality VP9 needs about 15% fewer bits on the phone, and about 40%
fewer at 720p on the desktop, where VP8 has less spare CPU to spend on quality.

Still to do:

1. **Live check.** Run a room with a Windows desktop and the phone both
   sending. Both must see each other, and the logs must show `vp9` at capture
   start. Force the fallback on a slow machine (or temporarily lower
   `BUDGET_MEAN_SHARE`) and confirm that receivers keep the picture across the
   switch.
2. **x86 SIMD.** x86 desktops still encode VP9 in plain C, so 720p needs two
   threads and a weak laptop will fall back to VP8. Enabling libvpx's x86 SIMD
   means adding nasm to every build machine and CI image, and then
   `Simd::X86` in `doubleslash-vpx/build.rs`.

### Installed simultaneous-use acceptance

`device-routing` is the default of `doubleslash-features` and advertises
`core.devices.v1`. Routing, device keys/registries, encrypted own-device
room-key handoff, and portable backups exist. This is **not** completed pairing
or continuous sync.

The installed desktop (normal packaged profile) and phone on one identity
passed the connected, room-chat, reconnect, third-contact, and relay checks on
2026-09-22 ([result](docs/DEVICES_AND_BACKUPS.md#simultaneous-identity-preview-testing)).
Still required before claiming the preview:

- **Incoming call answered on either device, stopping the ring on the other.**
  Not yet run live. The desktop's ringing dialog used to stay up after the call
  ended elsewhere and send a reject when it timed out; `CallAnsweredElsewhere`
  fixes that. Rerun on a build that includes it.
- **Restore edge cases.** A plain desktop → phone → desktop round trip works.
  Still to run: a large attachment, a missing attachment, a keyfile-protected
  source, a failed or cancelled document-picker copy, and a restart after
  profile selection. Check the actual history and files.

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
VP9/VP8 already cover encode; a backend only has to produce tightly-packed I420
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

---

## Phase 4 — Android distribution

App-side policy work for a first listing is in the tree (`targetSdk` 36,
`specialUse` FGS, terms gate, disclosures, Report, locked portal WebView,
incoming-call notifications). What is left is mostly Play Console, CI, and
release hardening.

1. **CI coverage.** `.github/workflows/android.yml` builds a debug APK on
   `develop` pushes and publishes `android-nightly`. It does not run Gradle
   unit tests or `cargo ndk` clippy (only `scripts/ci_local.*` does, when the
   toolchain is present), and it does not gate pull requests.
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

## Phase 5 — Client polish

1. **Android room per-peer grants** (`generateRoomInviteForPeer`) are not wired;
   shareable links carry a Space inclusion proof and no grant. Desktop has it.

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
- **Layer 2 — key hierarchy (entirely unbuilt).** Extends pairwise sender-keys
  `GroupKeySource` (TreeKEM declined). Scope: per-node epoch secrets; HKDF
  inheritance down `inherit=true` edges; `inherit=false` compartments as their
  own pairwise-distributed group; real values for reserved `inherit` /
  `key_commit` / invite `space_node_key`; capability-token admission under the
  node key without changing the admission call shape. Confirm before freezing
  `v1` hash labels: a `SpaceGrant` targets exactly one `node_id` and proofs
  don't inherit.

Space non-goals: literal MTC / X.509; subtree delegation in v1 (owner-signed
delegation leaves are a compatible later addition); a separate node `kind` such
as `Closet` (sub-rooms are Space children via `parent_id`); inter-cluster
federation, blinded tokens, payments.

### Post-quantum crypto

Not built; the codebase is fully classical. The 2026-07-11 assessment, role
table, findings, and build order are in
[docs/POST_QUANTUM.md](docs/POST_QUANTUM.md). First step if picked up: hybrid
`X25519MLKEM768` TLS via the `aws-lc-rs` rustls provider (transport only; do not
market as “PQ-ready”).

### Relay discovery (only if demand appears)

Only once relay sets grow past a hand-managed cluster. The invite-only trust
root stays intact; neither is a user directory.

- In-band capability gossip of supernode bundles.
- Signed `RelayAd` + capacity-aware selection.

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
- **Inter-cluster federation or a DHT.** Linking nodes into one cluster is the
  scaling model; cross-operator federation reopens discovery.
- **Stereo / spatial voice mixdown.** Voice is mono end to end.
- **Polyphase resampling.** Nobody has reported a linear-interpolation
  artifact. Reopen only on one, with a recording.
- **Mic on the media timeline (talking-head lip sync).** The microphone stays
  off the synced PTS timeline; ±40–80 ms audio-led sync covers shared content.
- **Durable first-party portal documents.** Demo state lives in open pages;
  closing the last page loses it. Collaborative-tool durability is an app
  problem, not a relay cache.
