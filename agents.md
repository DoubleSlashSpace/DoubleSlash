# Agents.md

## Overview

This file defines working rules and regression-sensitive invariants for DoubleSlash (`doubleslash-*`, D://). Use the implementation to verify current behavior; these rules constrain changes, but are not evidence that a feature has passed acceptance testing.

DoubleSlash is a client-owned, invite-only peer-connectivity framework. The desktop Qt/QML client and Android Kotlin/Compose client share a Rust core. Direct sessions use QUIC; selected supernodes provide relay, ephemeral SFU rooms, and in-app portal hosting. WebSocket carries signaling and membership and can also carry fallback room audio. There is no first-party identity, discovery, or presence backend.

User-facing setup belongs in [README.md](README.md), architecture in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), and open work in [backlog.md](backlog.md). Before working inside the supernode-manager workspace, read its [agents.md](rust/doubleslash-supernode-manager/agents.md) for the operator contract.

## Agent Roles

These are review responsibilities, not a requirement to create separate agents. Apply the relevant roles to the task.

| Role | Responsibility |
|---|---|
| Project manager | Track scope, acceptance gaps, lockups, races, and call reliability. Distinguish implemented, automatically tested, and manually validated behavior. |
| Signaling / handshake developer | Maintain invite persistence, signed handshake/signaling, replay controls, endpoint discovery, and deterministic reconnect transitions. |
| Transport / feature developer | Maintain direct and relay channels, capability negotiation, auth, quotas, and media budgets. Treat the UI as a framework consumer. |
| Security reviewer | Review identity/device trust, encryption boundaries, room key installation, plugin trust, release verification, and negative paths. |
| QA | Run focused tests and lint, then broader checks where warranted. Exercise reconnects, duplicate messages, profile isolation, and two-client media flows. |
| Operations | Maintain supernode packaging, manifests, relay/SFU/portal hosting, and cluster deployment through the manager. Preserve identity and trust during redeploys. |
| Documentation | Update behavior, defaults, privacy disclosures, and architecture diagrams in the same change. Verify claims against consuming code. |
| UX/UI | Keep chat, voice, rooms, media controls, permissions, and local data actions consistent across the relevant client surfaces. |

## Global Guardrails

- Keep identity and trust client-owned and invite-only. Supernodes are transport/hosting peers, never identity authorities. Do not introduce mandatory central services.
- Represent cross-peer application features through stable capability IDs with auth and quota policies. Do not bypass negotiation or authorization for UI convenience.
- Keep supernodes opaque to private application content. Do not add payload logging, decryption, disk persistence, or store-and-forward caching. Routing metadata and operator-hosted portal assets are separate from participant content.
- Preserve restart safety, bounded queues, and real-time media budgets. Do not block a manager event loop waiting for an event that the same loop must process.
- Pair meaningful behavior changes with tests or reproducible validation. Do not report builds, deployments, or manual acceptance without evidence from the current task.
- Fix warnings at their source. Do not add `#[allow(...)]`, `-A` flags, ignored tests, or unchecked unwraps merely to make checks pass. If a suppression is justified, use narrowly scoped `#[expect(lint, reason = "...")]` and explain the actual reason.
- Binary-crate reachability differs from library visibility: `pub` does not make unused supernode code live. Check normal and test builds before suppressing test-only dead code. Do not assume existing crates are suppression-free.
- After Rust edits, format and verify every affected workspace as described under [Validation](#validation).

## Architecture Notes (Agent Contract)

### Source map

Unless stated otherwise, client paths below are relative to `rust/doubleslash-client/src/`.

| Area | Source |
|---|---|
| Identity, devices, local stores | `identity.rs`, `device.rs`, `device/store.rs`, `peer_store.rs`, `chat_store.rs`, `room_store.rs`, `backup.rs`, `backup/` |
| Transport and signaling | `connection_manager/`, especially `manager/{inbound,routing,invite,peer_session,room_session,device_session,device_calls,video_session}.rs`; `quic_relay_client.rs`, `quic_tls.rs` |
| Calls and media | `call_controller.rs`, `video/`, `content_capture.rs`, `content_sender.rs`, `content_playout.rs`, `content_audio.rs`, `media_clock.rs`, `media_sync.rs`, `group_key.rs` |
| Desktop UI | `ui/bridge.rs`, `ui/settings_model.rs`, models in `ui/`, and `rust/doubleslash-client/qml/` |
| Android | `rust/doubleslash-android/src/{lib,command,event,session,video}.rs` and `android/app/src/main/java/com/doubleslash/client/` |
| Capability runtime | `rust/doubleslash-features/src/{descriptor,registry,quota,client_modules,wellknown,channel_frame,channel_tag,loader}.rs` |
| Supernode | `rust/doubleslash-supernode/src/{main,relay,signaling,sfu,manifest,cluster_link}.rs` |
| Installer / packaging | `rust/doubleslash-installer/`, `build_win64.ps1`, `build_linux.sh`, `build_macos.sh`, `scripts/build_supernode.*`, `.github/workflows/` |

### Identity, signaling, and reconnects

- Invite handshakes require ephemeral X25519 material, signature verification, transcript binding, and expiry checks. Do not restore the empty session-key compatibility path.
- Post-handshake signaling requires Ed25519 verification and timestamp freshness (`MAX_MESSAGE_AGE_SECS`, currently 300 seconds), plus per-sender signature replay deduplication. `SfuAudio`, `FileTransferChunk` / `FileTransferComplete`, and `SfuFileChunk` / `SfuFileComplete` skip deduplication, not signatures or freshness. Preserve negative tests for replayed, stale, and future-dated messages.
- Preserve the mutual-trust receiver gate for all chat/call/file message classes, including call accept/end and file payloads. An authenticated relay connection alone does not authorize a peer to send application traffic.
- Normalize padded/unpadded Ed25519 public IDs where identity equality or ordering matters, including SFU ACLs and group-key election. A hex peer-store key is not a `public_id`.
- Direct video/content-audio signatures bind `direct_conv_id(sender_public_id, recipient_public_id)`. Resolve the recipient through `recipient_public_id` in `manager/peer_session.rs`; signing against the `peers` map's hex key makes the receiver reconstruct a different conversation ID.
- Classify new supernodes from the signed invite's `is_supernode`, persisted as `is_supernode` / `supernode_from_invite`. Do not infer supernode identity from `relay_hints` or a WebSocket URL. Existing `restore_supernodes_referenced_by_ids` recovery uses saved room references; it is not a general endpoint heuristic.
- Client UDP hole punching is implemented in `manager/peer_session.rs` (`request_hole_punch`, `handle_punch_ready`). Preserve the connected-supernode and known-peer checks, endpoint validation, bounded scheduling delay, and registration expiry. Do not describe it as server-only scaffolding. This path does not use STUN.
- Direct-call fallback allows five seconds for direct QUIC before requesting a temporary private SFU room through a trusted connected node. Cancel pending fallback on recovery or call end.
- Supernode bootstrap replies (`RelayGranted`, `SupernodeInfo`) prefer the live WebSocket through `send_bootstrap_to_peer`, then QUIC if delivery fails. Ordinary signaling prefers QUIC. A stale prior-process QUIC writer must not swallow a new connection's bootstrap.
- Portal fetch readiness waits for `RelayClientReady` outside the manager loop, with a bounded timeout. Reconnect testing must include close/reopen without a cluster restart that could mask stale state.
- Preserve relay ticket renewal and the persisted endpoint mailbox used after address changes or restarts. Keep expiration and renewal windows aligned with their implementation constants.

### Devices and backups

- `device-routing` is a default feature of `doubleslash-features`; `DEVICE_ROUTING_READY` and `core.devices.v1` describe root-authorized endpoint routing. Keep clients and nodes compatible before registration. Do not equate this with completed device pairing or continuous sync.
- Preserve root/device TLS claim checks, separately tracked direct/relay routes, sibling-safe replacement/disconnect, per-endpoint room subscriptions, and one-answer call selection. A stale disconnect must not delete a replacement or sibling route.
- Own-device room-key handoff in `manager/device_session.rs` is encrypted and binds the device roster and a fresh challenge. Reject malformed, cleartext, late, or conflicting replies; a sibling sharing the identity must not bypass that gate.
- Registered signaling writers share the identity's 8 MiB queue budget; adding devices must not multiply the allowance.
- `DeviceTrustStore` persists encrypted accepted registries with transactional predecessor, revocation, rollback, and fork checks. Backup snapshots must preserve and validate this optional database. Local version checks cannot detect rollback of an entire profile to an old backup.
- `open_with_key` store APIs separate data access from signing authority, but delegated registry authentication and a full independent storage-key distribution scheme are not integrated into live sessions.
- Restore validates in staging and creates a separate selectable profile. Do not overwrite an existing profile or copy `device-key.dat`, keyring/Keystore entries, or native plugin binaries. Keep Qt settings pinned to the profile they loaded until restart.
- JNI session handles are registry IDs, not raw pointers. In-flight backup operations must retain valid store references when a session stops.
- File-based device moves still require quitting the source before connecting the destination. Same-identity acceptance targets the normal packaged desktop profile and phone, not the historical `.clientA` test identity. Do not claim installed simultaneous-use support until that acceptance is demonstrated. See [Devices and backups](docs/DEVICES_AND_BACKUPS.md).

### Rooms, Space trees, and history

- `RoomStore` owns encrypted definitions in `my_rooms.dat`, keyed by `(supernode_id, room_id)`. SFU hosting is in-memory only; do not add `sfu_rooms.json` or other room-definition persistence on nodes. User-created rooms expire after 900 seconds without voice participants or chat subscribers; the built-in `default` room is retained.
- **History uses a different key:** `chat_store::room_conversation_id` returns `room:{room_id}`. Desktop and Android must use this helper. Preserve the idempotent migration from legacy host-qualified keys; failover must not split history.
- Sidebar hiding is a local tombstone, queried through `is_hidden_from_sidebar`; it is not a field returned by `RoomStore::list()`. New consumers must filter/annotate it separately. Keep sidebar/host routing scoped by `(supernodeId, roomId)` even though history is host-independent.
- Reconnect rematerialization replays saved non-hidden definitions onto the live cluster host, with the original creator and retained invite credential. `CreateRoom { materialize_only: true }` must not auto-join voice; user create may auto-join. Re-run cluster replay when the verified roster arrives.
- Route room signaling through the correct node (`resolve_supernode_ws_target`), not the first connected node. Preserve signed join/create denials and rollback of optimistic UI state. Only transient `room_absent` uses bounded backoff; ordinary denials surface immediately.
- Private-room authorization comes from validated Space proof/grant, local allowed/token state, or creator admission. Client-held invite tokens can be re-registered after GC/restart. Room ID alone is not a credential. `RoomRoster` describes room existence and policy; `PeerAuth` describes peer trust. Neither is a private-room membership grant.
- Do not reintroduce cluster `RoomGrant`/private allowed-set replication. Cluster links propagate room rosters, Space roots, peer trust, and opaque room frames. Replicated chat/audio must be deduplicated and never re-replicated.
- Public/private creation policy is read from `room.audio.sfu` only (`manifest::sfu_room_creation_policy`); defaults are public off, private on. The similarly named video/content-audio params are not media restrictions. Verify a consuming check before documenting any descriptor parameter as enforcement.
- Nesting uses Space `parent_id`, not a new node kind. Keep the “Members can invite” toggle, `invite_policy`, signed roots, inclusion proofs, and grant validation aligned. Preserve equivocation detection and periodic root reannouncement. Reserved `inherit`, `key_commit`, and `space_node_key` fields do not justify changing existing `v1` hash labels.

### Supernode Opacity (Agent Contract)

Supernodes may use peer/device IDs, room/session IDs, membership, indices, signatures, message types, control state, timing, and wire-byte counts. Room names and file offer/completion metadata can be visible. They must not gain participant chat/file/media keys or inspect private content.

- Forward `EncryptedSignal`, sealed room payloads, and media fragments without decrypting them. Compute quotas from wire/ciphertext lengths. Do not log bodies, audio, file bytes, or filenames, or retain payloads for later replay.
- `game.relay.v1` forwards application datagrams without parsing them. This is opaque forwarding, not proof of end-to-end encryption: the SDK does not itself seal game payloads against the relay. Do not promise game confidentiality that the application has not implemented.
- QUIC provides transport encryption on direct peer sessions. Relayed direct datagrams use the pairwise key derived by `derive_pairwise_relay_key` from static identity keys; that construction does not provide forward secrecy. Distinguish it from ephemeral invite-handshake encryption.
- Room media uses `SenderKeysGroup` epoch keys. Preserve AAD separation: voice's existing bytes stay unchanged; video adds `0x02`, content audio `0x03`. The relay must not interpret media timestamps or codec payloads to make routing decisions.
- High-level room sends require real distributed keys (`may_send_room_e2e_content` / `has_real_key`); reject cleartext inbound room payloads and fail closed on failed opens. The low-level group-key API still has a deterministic epoch-0 fallback. Do not treat that API's ability to return a key as authorization to send private content.
- `accept_group_key_from` requires the current elected keyer. Election uses normalized public IDs and authoritative membership; it is not restricted to the room creator. First mint waits for another member. Preserve epoch checks: first install accepts the offered epoch; subsequent installs accept current or at most `MAX_EPOCH_ADVANCE` ahead using wrapping comparison. Epochs remain `u8`.
- Preserve group-key ACK/reseal recovery: 750 ms retries, at most 16 attempts, and bounded rearming for lagging members after the distribution window. Own-device handoff has its separate roster/challenge gate described above.
- Keyer-driven distribution alone does not cover a member that loses its in-memory epochs. Distribution fires on membership *edges* and on frames sealed under an old epoch; a member that restarts produces neither, because it never left the cluster-wide union and an unkeyed member fails closed on every room send. `SfuGroupKeyRequest` (member → keyer, `EncryptedSignal`-sealed, so supernodes forward it blind) is the recovery path — keep it, and keep it driven by both `sync_room_membership` and the retry tick, since a member that restarts into a quiet room gets no further membership updates. Its backoff must stay uncapped in attempts: giving up restores the strand it exists to fix.
- Serving a key request is authorized by room membership and election, not peer trust: the requester must be in the authoritative membership union and we must be the elected keyer *and* elected device, holding a real key. Hand out the current epoch only, and never alongside an in-flight distribution for the same member. Room-key message classes are deliberately outside the mutual-trust receiver gate — do not add them to it.

### File delivery

- Room offers are advertise-then-pull: chunks target the requesting member; late requests are served from the originator's disk, not a supernode cache.
- Preserve `xfer-{transfer_id}` chat IDs, offer withdrawal on deletion, and sender-authenticated `SfuFileRevoke`. Missing offers should fail visibly. Revocation cannot remove already downloaded copies.
- Files above `INLINE_MAX` (8 MiB) stream uncompressed and without delta encoding. Sparse receiver offsets depend on `payload_len == size`; changing encoding requires changing that framing/offset contract. Maximum transfer size is 250 MiB.
- Keep sender pacing, bounded queues, retries, stalled-transfer detection, and completion/hash checks. Never trade missing ordered chunks for silent success.

### Desktop UX

- Keep Peers limited to `list_non_supernode_peers()`; Rooms lists trusted `supernodes()`. Keep `connection_mode`, participant state, unread/missed-call badges, and tray behavior synchronized.
- Voice ends through `VoiceRail` End/Leave. `RoomPanel` is the text room; leaving voice must retain its selection. Sidebar removal is local hiding, not a remote delete or voice hang-up action.
- Wire room nesting/invite controls through `create_room_impl` / `create_sub_room` and `adopt_room_into_space`. Preserve expand/collapse for existing `parent_id` hierarchies.
- Re-enumerate share sources on every popup open. A missing saved source must show “Source unavailable,” not silently select another. Ask about audio when starting a share, not stopping it.
- Drive video UI from `video_active`, `content_audio_active`, and `video_preview_active`. Show an explicit unavailable state where a capture/encode path is absent.
- Keep avatar serialization in `avatar_config.rs`, SettingsModel, and `Avatar.qml` aligned; exchange custom configs only after trust. Keep the core avatar module available without Qt.
- Keep privacy controls wired to the actual ChatStore trim/purge methods and identity-keyring deletion. Every CXX-Qt qproperty needs a matching Rust field and initialization; headless checks alone do not validate the Qt build.

### Android boundary

- The JNI bridge uses four lifecycle/command methods **plus** `nativeSubmitCameraFrame` for CameraX buffers. Add ordinary features through the JSON command/event channel; keep literal event wire names stable. Camera frames use the dedicated buffer interface, not JSON.
- Never serialize incoming per-frame audio/video/content-audio events into UI JSON. `session.rs::route_media` must route direct and room voice into `CallController`; simply filtering events would produce silent calls. Incoming video/shared-audio consumers are still missing. Portal datagrams are queued for the portal bridge separately.
- Keep the dedicated event OS thread attached to the JVM; do not turn it into a migrating Tokio task that reattaches for each event.
- Set room mode before starting room audio; clear it before stopping. Leaving a voice room must stop capture. Preserve incoming-call notifications and Answer/Decline handling when the activity or ViewModel is absent.
- Keep the unlocked-session foreground service as `specialUse`; claim microphone/camera types only for an active call with the matching permission. Preserve the notification Disconnect action, terms acceptance, explained runtime permissions, and identifier-only share-sheet reporting.
- Never navigate to external HTTPS content with the native portal JS bridge attached. Keep file/content-provider access disabled in the portal WebView.
- Keep CameraX-to-Rust sending separate from unsupported-platform `NullCamera` fallback. Android is no longer a platform with no camera implementation.

### Media invariants

- Camera implementations cover Windows, Linux, macOS, and Android. Screen/window capture and WASAPI shared-audio capture remain Windows-only. The macOS camera uses an ARC Objective-C shim; preserve a compiling empty-device fallback on unsupported targets.
- Retain VP8 on every platform. Advertise only codecs the build can encode and decode (`available_codecs`); never advertise test `Stub`. Windows also has Media Foundation H.264. Do not add an in-tree AVC encoder without licensing review.
- Capability IDs are codec-independent. Direct calls negotiate a common codec; rooms send one codec chosen by the sender. Decoders are per `(sender, codec)`. A decoder change must not reuse another codec's reference state.
- `video/fragment.rs` owns framing (`FRAGMENT_VERSION = 0x03`). Codec and PTS are signature-bound; unknown codecs and mixed-frame metadata must be rejected. Preserve frozen codec bytes in `rust/doubleslash-features/src/video_codec.rs`.
- Picture-in-picture is composited before encoding, into one stream per peer. Room video is relay-datagram-only; do not add a WebSocket media envelope. WebSocket-only members can retain room voice.
- `SessionMediaClock` stamps video and shared audio at capture. Content offsets come from the capture device (`CaptureTimeline`), not frame counts: a quiet loopback device emits no packets, but time still passes. Preserve resampler phase across capture reads.
- Shared-audio playout anchors video per sender. Hold/drop against that sender's timeline; free-run when no anchor exists or it is stale. Do not compare different senders' PTS or slave video to the microphone stream, which has no PTS.
- Preserve content-audio prebuffering, bounded backlog, and rebuffering without advancing `next_seq` past in-flight frames. Resume from the buffered head after refill. Concealment is bounded (currently 400 ms), so stopped audio cannot keep a phantom sync timeline alive.
- Shared audio follows the whole viewer set from `setContentAudioViewers`, with independent voice/content gains. Continue decoding silent content slots to maintain codec/sync state; muting voice must not mute the presentation.
- A dry voice queue is not immediately packet loss. Count a bounded short gap only when that sender resumes; discard intentional longer silence. Room ABR's inbound-loss proxy is not receiver feedback for the local outgoing stream.
- Video ABR uses the connection-statistics tick, stays below the configured ceiling, and keeps room to adapt downward in every preset. Read thresholds and bitrate floors from `video/sender.rs` instead of maintaining a second tuning table here.

## Using the Modular Framework (Agent Contract)

### Registration, authorization, and dispatch

- Descriptors define ID, version, kind, params, auth, and experimental status. Compatibility uses ID plus major version. Use `wellknown.rs` and `client_modules.rs` as the descriptor sources.
- `register_module` registers a descriptor and implementation; `bind_module` attaches an implementation to an existing manifest descriptor. Preserve operator parameters when binding. Use the opt-in [matchmaker example](rust/doubleslash-features/src/examples.rs) as a template rather than duplicating an example here.
- **Registry dispatch is not an authorization boundary.** `dispatch_message` checks module presence and inbound quota. `dispatch_invoke` calls the module directly. `dispatch_invoke_datagram` checks kind/quota and allocates a tag. None of these establishes peer trust or room membership by itself.
- Preserve authorization at transport/invocation entry points. Client `handle_capability_invoke` in `manager/inbound.rs` checks the peer's announcement, local descriptor auth, and `FeatureTrustGate` before dispatch. Keep equivalent trust/membership checks on other entry paths; do not assume a lower-level registry helper supplied them.
- First-party `core.*`, `transport.*`, `room.*`, `web.*`, and `game.*` namespaces bypass the bespoke consent prompt, not transport authentication or auth-tier checks. Other namespaces require stored consent for the `(feature, peer)` pair.
- Hosted declarations belong in `supernode.toml`; extend its schema rather than adding ad-hoc feature environment toggles. Missing manifests load first-party defaults; built-in core/room/game descriptors are also registered for quota/routing classification. Omitting a descriptor is not necessarily a traffic-disable switch.
- Native modules require signature/trusted-signer checks but execute in-process. Do not describe the loader as a sandbox or assume hot reload is wired simply because registry replacement exists.

### Auth + quota enforcement

- Use `gate_through_feature` for outbound chat/files and quota-checked media send helpers. Hot receive paths that bypass `dispatch_message` still require `gate_inbound_through_feature` or the appropriate fan-out helper.
- Maintain separate inbound/outbound buckets per `(feature, peer)` and clear both on the relevant disconnect. “Symmetry” means both directions are enforced, not identical numeric rates: room-video senders and fan-out recipients have distinct descriptor limits. On device-aware paths, do not clear a sibling's active state prematurely.
- `quota_bytes_per_sec_outbound` / `quota_datagrams_per_sec_outbound` override outbound rates; otherwise they inherit inbound rates. Missing rates default to 64 KiB/s and 256 datagrams/s. A zero rate is an unbounded sentinel; do not introduce it as a workaround for pressure.
- Read current video quota constants from `wellknown.rs`. `ROOM_VIDEO_CONCURRENT_SENDERS` sizes a budget; it does not enforce a sender-count limit.
- The deliberate ordered-file exception is supernode forwarding of `SfuFileChunk` / `SfuFileComplete`: observe quota refusal without dropping those payloads. Retain `FileFrameLimiter` on both signaling transports, the 256 KiB message cap, and the 8 MiB recipient/identity queue bound. Offers, requests, and revokes stay quota-gated. Do not generalize the exception.

### Channel-tag rules

[Channel framing](rust/doubleslash-features/src/channel_frame.rs) is shared by Rust and [web-sdk/doubleslash.mjs](web-sdk/doubleslash.mjs). Keep fixed tags and SDK helpers aligned; allocate dynamic tags through the registry (`0x10`–`0xEF`), with `0xFF` reserved for broadcast.

| Tags | Use |
|---|---|
| `0x00`–`0x03` | Control, voice, chat, files |
| `0x04` / `0x05` | Room voice / game relay |
| `0x06` / `0x07` | Direct / room video |
| `0x08` / `0x09` | Direct / room shared audio |

Direct peer stream frames must be tagged; do not restore untagged JSON compatibility. Room video/content-audio tags remain outside application-content classification at the relay. Media descriptors advertise capability/quota while dedicated paths carry the bytes; this module shape is not a product-readiness claim.

## Feature Module Reference (Agent Contract)

Keep the catalogue in [wellknown.rs](rust/doubleslash-features/src/wellknown.rs), implementations in [client_modules.rs](rust/doubleslash-features/src/client_modules.rs), and the reader-facing summary in [README.md](README.md#built-in-capabilities). Do not duplicate quota tables that drift from those constructors.

The current families are `core.chat.v1`, `core.file.v1`, `core.audio.opus`, `core.video.v1`, `core.audio.content.v1`, `core.devices.v1`; room chat/file/audio/video/content-audio; `web.host.app.v1`; `game.relay.v1`; and `transport.quic.*` descriptors. Bespoke modules use `x.<vendor>.*`.

The portal uses `WebAppRequest` / `WebAppResponseHeader` framing from [web_app.rs](rust/doubleslash-features/src/web_app.rs) over authenticated QUIC bidi streams. Keep native `d://` navigation, the Chromium `doubleslash://` alias, and the `window.doubleslash` bridge aligned. Do not reintroduce public HTTP/WebTransport game listeners or page-side SFU clients. Portal assets and game reuse/testing instructions belong in the [operator guide](docs/SUPERNODE.md) and [game guide](games/README.md).

## Build Gotchas (Agent-Relevant)

- There are four Rust workspaces: `rust/` (features, supernode, installer, Opus, VP8), `rust/doubleslash-client/`, `rust/doubleslash-android/`, and `rust/doubleslash-supernode-manager/`. Run commands in the intended workspace; `--manifest-path` does not make Cargo read that directory's local `.cargo/config.toml` when invoked from elsewhere.
- Initialize both codec submodules. Opus uses CMake and default DNN weights from `scripts/fetch_opus_weights.ps1` / `.sh`. With CMake 4, preserve `CMAKE_POLICY_VERSION_MINIMUM=3.5` where configured.
- VP8 builds through the wrapper's `build.rs`, Perl RTCD generation, and parsed libvpx `.mk` source manifests. Do not replace this with source globs that include VP9. Generic C builds currently avoid assembler/SIMD; changing that requires matching architecture flags and build tooling.
- Cross-compiling `build.rs` must use `CARGO_CFG_TARGET_OS`, not host `cfg!`. Android/Bionic must not link a separate `pthread` library.
- Preserve Android `-Wl,--no-undefined` and the `c++abi` link alongside the static C++ runtime. Keep the deliberate weak `getrandom` probe compatible with older supported APIs.
- Qt UI requires Qt 6 (`QMAKE` / `CMAKE_PREFIX_PATH`); WebEngine is an optional additional feature. Linux camera bindings need libclang. Headless checks cannot validate QML, CXX-Qt properties, or native platform capture.
- Android Gradle drives cargo-ndk. Keep `doubleslash.ndkApi` equal to `minSdk`; current defaults are API 26 and ARM64. Use forward slashes in `android/local.properties` SDK paths. Read tool versions from Gradle/configuration and [docs/ANDROID.md](docs/ANDROID.md), not a duplicated policy deadline.
- `aec` remains optional/experimental; DSP unit coverage is not real-device echo-cancellation acceptance. `lint-macos` type-checks the Rust camera wrapper on other hosts; it does not validate the Objective-C shim or hardware.
- Keep client/installer release versions and platform metadata aligned; use `scripts/check_version_sync.ps1`. Supernode PE metadata is derived from its package version.
- `run_client.bat` launches the packaged executable with `.clientA`; profile-isolated tests must launch the binary directly with explicit profile directories. Do not use development launchers to infer the user's normal profile.

## Documentation and release verification

- Update [PRIVACY.md](PRIVACY.md) and its README summary whenever capture adds a platform/source or widens defaults. Preserve the disclosures that whole-screen sharing includes overlays/notifications and pre-build-20348 Windows application audio falls back to whole-machine audio.
- Match README desktop defaults to SettingsModel qproperties and defaults. Keep Android preferences separate. Update the architecture diagram with new crates, capture backends, or wire tags.
- Do not claim encryption, quota enforcement, consent, or platform support from a descriptor or comment alone; inspect the calling path. Keep implemented behavior separate from remaining acceptance work.
- Installer stable-release manifests, when supplied, require the project Ed25519 signature and matching archive hash. A stable install can currently proceed without a manifest; nightly installs require a manifest but accept it unsigned. Published SHA-256 sidecars are checked when present. Do not document unconditional signed-manifest enforcement.
- Release automation supports conditional SignPath, Apple signing/notarization, and artifact attestations. Do not infer configured credentials, program approval, signing status, or live deployment state from workflow support.
- Keep third-party CI actions pinned to immutable commit SHAs and retain supply-chain checks when changing workflows.
- Distribution notices are generated from the resolved dependency graph by [scripts/generate_licenses.mjs](scripts/generate_licenses.mjs), which also copies the target's reviewed supplement from `packaging/licenses/<target>/<product>/` and records both in `inventory.json`. [verify_artifact.mjs](scripts/licenses/verify_artifact.mjs) then re-checks the *final* archive and fails on a missing or empty notice; the desktop build scripts already invoke it. Keep both steps when changing packaging.
- Notice generation is not an approval gate. The reviewer signature, build-input binding, per-binary hash inventory and `review.json` were removed deliberately on 2026-09-19 because none was a licence obligation. Do not reintroduce them. `collect_qt_license_evidence.mjs` remains a manual evidence tool and is wired into no build.
- Qt and FFmpeg ship under LGPL and Qt WebEngine embeds Chromium, so the desktop build owes a prominent in-app notice, the licence texts, and replacement instructions. The Settings > About “Third-Party Notices” card is that notice; it resolves the packaged directory through `third_party_notices_path`. Keep every LGPL library dynamically linked and replaceable, and keep `relinking-instructions.txt` true for the layout you ship.
- Adding a bundled non-Rust component means adding its notice to that target's supplement. Nothing detects an omission: `verify_artifact.mjs` only proves that what `inventory.json` promised actually shipped. The Chromium snapshot's third-party credits are generated by [scripts/licenses/chromium_credits.py](scripts/licenses/chromium_credits.py) from the qtwebengine-chromium revision the bundled Qt pins; re-run it when that Qt version changes. See [docs/LICENSING.md](docs/LICENSING.md#known-gaps).
- Use the in-tree `sign-release-manifest` helper and final packaged archive hashes. Its public key is in `keys/release-signer-public.pem` and installer `release_manifest.rs`; do not invent a key-rotation protocol. Peer build attestations are signed claims, not proof of unmodified remote execution.

## Validation

For each affected Rust workspace, run `cargo fmt --all`, then `cargo fmt --all -- --check`. Run focused `cargo test` and `cargo clippy -- -D warnings` for touched crates. Changes spanning workspaces or installer/release paths need the applicable broader local CI checks. Diagnose failures before hand-off; report environment blocks and manual checks not performed.

- Before changing quota, dispatch, signaling, or capability behavior, establish a relay/SFU/room test baseline and a two-client baseline. Repeat the affected flows after the change, including reconnect and denial paths.
- Run a Qt-enabled build for desktop bridge/QML integration changes. For Rust macOS-camera changes, include `cargo clippy --no-default-features --features lint-macos -- -D warnings` from the client workspace; native shim checks require macOS.
- Use [scripts/ci_local.ps1](scripts/ci_local.ps1) or [ci_local.sh](scripts/ci_local.sh) for host checks across all four Cargo workspaces, including the Android bridge. Gradle unit tests and `cargo ndk` clippy run when those toolchains are present and are skipped (with a reason) when they are not. Linux-native client cfg and supernode packaging are still not produced from the Windows script — use `ci_local.sh` on Linux/WSL and [docs/ANDROID.md](docs/ANDROID.md) for device/APK work. Verify script scope rather than assuming every target is covered.
- Measure current test inventory with `cargo test -- --list` when counts are needed. Do not reuse dated totals as sign-off. Coverage scripts report LLVM line/region coverage; use focused features/supernode scopes for protocol regressions and report the actual result.
- Documentation-only changes require source verification, local-link/anchor checks, and `git diff --check`; no Rust test run is implied by editing prose.

## Roadmap & Status

Maintain open work and acceptance criteria in [backlog.md](backlog.md), not a second milestone history here. Keep this file's durable invariants current when behavior changes. Remove completed status items instead of appending dated “fixed” narratives.

Current boundaries that affect agent claims:

- A/V sync and adaptive bitrate are implemented. Windows has camera, screen/window, and shared-audio capture; Linux/macOS camera hardware validation and non-Windows screen/shared-audio capture remain gaps. Android has camera sending but lacks incoming video/shared-audio consumers. Full media acceptance still needs physical-device/two-client/room checks.
- Device routing, device keys/registries, and portable backups exist. Pairing, delegated live authorization, continuous local-data sync, remaining endpoint-state integration, and installed simultaneous-use/restore acceptance are unfinished.
- Space Layer 1 supplies signed trees and admission proofs; remaining Space crypto work is in the backlog. Native plugin sandboxing is not implemented.
- Portal demo state is ephemeral. Browser fixtures do not establish real-device QUIC acceptance; Focus Timer also assumes synchronized device clocks.
- Use the supernode manager for cluster operations. Preserve cluster configuration and member-restricted firewall rules during redeploy; run `cluster-sync` when membership changes. Consult the manager inventory and live checks for actual deployment status rather than treating historical acdc/ac1 results as current.
