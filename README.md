# DoubleSlash

[Website](https://doubleslash.space) · [Releases](https://github.com/DoubleSlashSpace/DoubleSlash/releases) · [Privacy policy](PRIVACY.md)

DoubleSlash is an invite-only peer-connectivity framework with desktop and Android clients. It provides chat, voice, file transfers, rooms, and media sharing through negotiated features. Identity and trust are stored on the device; there is no central account, peer directory, or presence service.

Peers connect directly over QUIC or through chosen supernodes. Supernodes provide relay transport, group rooms, and in-app portal hosting. Chat, file contents, voice, and video are encrypted between participants; relays can still see routing and membership metadata.

## Features

### Chat, voice, and files

- Direct and room chat with local history and unread counts. Direct chat includes delivery states and typing indicators. Room history is keyed by room ID, so changing the hosting supernode does not create a new conversation.
- Opus voice calls with push-to-talk, voice activation, noise suppression, and jitter buffering. Desktop controls include device selection, per-peer volume, and mute.
- File transfers up to 250 MiB. Files over 8 MiB stream from disk. Room files are advertised first and sent to members who request them; the sender must remain available. Revoking an offer stops further downloads but cannot remove copies already received.
- Local peer blocking, identity-derived avatars, and encrypted peer and room stores.

### Video & Screen Sharing

The desktop client includes video tiles, a pop-out view, local preview, quality controls, adaptive bitrate, and picture-in-picture compositing into one outgoing stream. Shared audio has its own volume/mute path and is synchronized with video.

| Platform | Implemented capture | Video codecs | Current limits |
|---|---|---|---|
| Windows | Camera, screen/window, system or application audio | Media Foundation H.264, VP8 | Full media path implemented; the remaining product acceptance checklist is in [backlog.md](backlog.md) |
| Linux | V4L2 camera | VP8 | Camera hardware validation pending; no screen/window or shared-audio capture |
| macOS | AVFoundation camera | VP8 | Camera hardware validation pending; no screen/window or shared-audio capture |
| Android | CameraX camera, VP8 sending | VP8 | Incoming video and shared-audio playback are not connected to a consumer yet |

Direct video negotiates a common codec. Room senders choose their own codec; a receiver without that decoder cannot display the stream. Room video requires QUIC relay datagrams. Room voice also has a WebSocket fallback.

### Rooms

- Supernodes host SFU (Selective Forwarding Unit) rooms for voice, chat, files, and video. The local room limit is 32 participants.
- Clients save room definitions in encrypted `my_rooms.dat` and replay them when reconnecting. Supernode room state is held in memory; user-created rooms are removed after about 15 minutes without voice participants or text subscribers. The built-in `default` room is retained.
- The Rooms sidebar supports nested rooms and private-room invitations, including a “Members can invite” option. Removing a room from the sidebar hides it locally. Leaving voice keeps the text room selected.
- Public user-created rooms are disabled by default. The `room.audio.sfu` capability's room-creation policy controls this; operators can enable them.
- Signed Space trees and membership proofs support private-room admission across a supernode cluster. Cluster forwarding carries room chat and audio between nodes.

### In-App Supernode Portal & Browser Games

The portal loads `d://<supernode_id>/…` through `web.host.app.v1` over the authenticated QUIC session. Desktop uses Qt WebEngine; Android uses WebView. Chromium uses `doubleslash://` internally to avoid interpreting `d:` as a Windows drive.

Portal games use `game.relay.v1` and the `window.doubleslash` channel bridge. There is no public HTTP/WebTransport game listener. Pages run inside the native client.

Seven demos are bundled: Presence Playground, Brick Breaker, Shared Canvas, Task Board, Focus Timer, Four in a Row, and Memory Match. Their shared state lives in open pages. See the [portal app guide](games/README.md) and [JavaScript SDK](web-sdk/doubleslash.mjs) for paths, examples, and limitations.

### Desktop and Android clients

The desktop UI uses Rust, Qt 6/QML, and CXX-Qt. It includes onboarding, invite links and QR codes, tray notifications, privacy controls, and an optional Ollama assistant (`x.ollama.v1`). Qt WebEngine enables the portal and inline video playback.

Android uses Kotlin/Compose with the same Rust core through JNI. It includes peer and room chat, voice, files, portal access, backups, and camera sending. An unlocked session runs a foreground service with a Disconnect notification action; incoming calls have notification controls. See [Android development](docs/ANDROID.md).

### Known limitations

- Direct connectivity depends on network reachability. UPnP and coordinated hole punching cannot traverse every NAT or firewall; use a mutually trusted supernode for relay when direct connections fail.
- Media support differs by platform as listed above. Cross-platform and two-client/room acceptance work remains in [backlog.md](backlog.md).
- Device-aware routing is enabled by default and advertises `core.devices.v1`, but pairing, continuous history sync, and installed phone/desktop simultaneous-use acceptance remain unfinished. Use backups for device moves and quit the source device before connecting the destination. Clients and nodes used by an identity must agree on `device-routing` support.
- Native feature plugins run in-process. A WASM sandbox is planned, not implemented.

## Quick Start

1. Install or build a client and create an identity with a local passphrase.
2. Create an invite and share the link or QR code with a peer.
3. The other peer pastes it into the Join dialog. A successful signed handshake adds the peer to the local trust store.
4. Select the peer to chat or start a call. To use rooms or relay connectivity, join a supernode using its operator's invite.

Invites use `https://doubleslash.space/i#…`. The fragment is not sent in the HTTP request; the landing page reads it locally and hands it to the installed client through `doubleslash://`. Invites can also be pasted directly into the app.

## Platform Support & Installation

The repository contains these packaging targets. Consult the [release assets](https://github.com/DoubleSlashSpace/DoubleSlash/releases) for a particular build's downloads.

| Platform | Package / build path |
|---|---|
| Windows | Installer or portable `DoubleSlash` folder (`build_win64.ps1`) |
| Linux | AppImage (`build_linux.sh`) |
| macOS | `.app` / `.dmg` (`build_macos.sh`) |
| Android | APK / AAB; Android 8.0+ (API 26), ARM64 by default |

On Windows, run `doubleslash-installer.exe` or extract the portable archive and run `DoubleSlash.exe`. The installer supports `--silent` and `--uninstall`. On macOS, open the DMG and drag the app to Applications. On Linux, mark the AppImage executable and run it. Android build and signing instructions are in [docs/ANDROID.md](docs/ANDROID.md).

### Uninstalling

- **Windows installer:** use Installed apps or `doubleslash-installer.exe --uninstall`.
- **Windows portable:** delete the extracted application directory. The client can register URI handlers on launch, so portable use is not guaranteed to leave the registry untouched.
- **macOS:** move the application to Trash.
- **Linux:** delete the AppImage. If installed separately, remove `~/.local/share/applications/doubleslash.desktop` and refresh the desktop database.
- **Android:** uninstall through system settings; this removes app-private identity and history data.

Desktop application removal does not by itself remove the profile. Export a backup before deleting identity data; see [Data and Files](#data-and-files).

## NAT Traversal

The client attempts direct QUIC connections using known endpoints and can ask a connected trusted supernode to coordinate UDP hole punching (`PunchRegister` / `PunchReady`). The node pairs registrations and supplies endpoints and a shared dial time. There is no STUN service in this path. See [peer_session.rs](rust/doubleslash-client/src/connection_manager/manager/peer_session.rs).

WebSocket signaling candidates are deduplicated and ordered with connected/known supernode endpoints first, then the peer endpoint, LAN hint, and further relay hints. This signaling order is separate from the media transport.

Desktop UPnP can map ports when the router supports it and the setting is enabled. It does not guarantee reachability through carrier NAT or restrictive firewalls. The default direct QUIC listener is UDP `61045`.

Rooms require a supernode. Direct one-to-one sessions do not, provided the peers can reach each other. Direct calls that fail to connect can fall back after five seconds to a temporary private SFU room on a trusted supernode. Relay tickets renew automatically, and the supernode endpoint mailbox retains reconnection information for 24 hours.

## Running a Supernode

The standalone supernode does not require Qt. Release workflows package Linux x86_64, Linux ARM64, and Windows x86_64 binaries with SHA-256 sidecars. Deployment, systemd configuration, portal templates, and clustering are covered in the [operator guide](docs/SUPERNODE.md). The [supernode manager](docs/supernode-manager.md) provides provisioning, deployment, cluster synchronization, and remote commands.

Build from the repository root:

```sh
cargo build --manifest-path rust/Cargo.toml --release -p doubleslash-supernode
```

Run on Linux/macOS:

```sh
export DOUBLESLASH_HOME="$HOME/.doubleslash-supernode"
export supernode_host="relay.example.com" # replace with your reachable host/IP
./rust/target/release/doubleslash-supernode
```

Run on Windows (PowerShell):

```powershell
$env:DOUBLESLASH_HOME = "$env:USERPROFILE\.doubleslash-supernode"
$env:supernode_host = "relay.example.com" # replace with your reachable host/IP
.\rust\target\release\doubleslash-supernode.exe
```

Use a separate data directory from your client. The node generates an identity and reusable invite on first launch and prints the invite for peers to join.

| Setting | Default | Purpose |
|---|---|---|
| `supernode_port` | `3478` | QUIC UDP listener |
| `supernode_signaling_port` | `34935` | WebSocket TCP listener |
| `supernode_host` | unset | Reachable host/IP included in invites and relay tickets |
| `supernode_invite_ttl` | `-1` | Invite lifetime in minutes; `-1` means no expiry |
| `supernode_access_mode` | `open` | `open`, `tos`, `code`, or `ad` (timer gate) |
| `supernode_sfu` | `true` | Enable the SFU room manager |
| `DOUBLESLASH_HOME` | `~/.doubleslash` | Node data directory |

Allow/forward UDP `3478` and TCP `34935`, or the configured ports. `listen_addr` and `ws_listen_addr` in `supernode.toml` override the environment listener addresses. A cluster also needs its configured member-to-member connectivity; use the operator guide or manager for those rules.

Gated nodes use an in-app portal for terms, access-code, or timer approval. Portal-only access precedes full relay access. No wallet or payment system is built into the client.

### Enabling Features on a Supernode

`<data_dir>/supernode.toml` declares hosted capabilities, listener overrides, and optional cluster settings. When absent, the node loads first-party defaults. Built-in core, room, and game descriptors are also registered for traffic classification and quota accounting; omitting an entry is not a blanket traffic-disable switch.

For the schema and configuration examples, see [manifest.rs](rust/doubleslash-supernode/src/manifest.rs) and the [operator guide](docs/SUPERNODE.md).

## Architecture

| Component | Responsibility |
|---|---|
| [doubleslash-client](rust/doubleslash-client/) | Shared identity, stores, transport, calls, media, and desktop UI |
| [doubleslash-android](rust/doubleslash-android/) + [android](android/) | JNI bridge and Kotlin/Compose UI |
| [doubleslash-features](rust/doubleslash-features/) | Capability descriptors, negotiation, auth tiers, quotas, channel framing, module loading |
| [doubleslash-supernode](rust/doubleslash-supernode/) | QUIC relay, WebSocket signaling, ephemeral SFU rooms, portal and game hosting |
| [doubleslash-opus](rust/doubleslash-opus/) / [doubleslash-vpx](rust/doubleslash-vpx/) | Vendored Opus and VP8 codec wrappers |
| [doubleslash-installer](rust/doubleslash-installer/) | Release download, verification, installation, and repair |
| [doubleslash-supernode-manager](rust/doubleslash-supernode-manager/) | Cluster deployment and operations |

Direct and relay transport use `quinn`/`rustls`. Signaling uses signed JSON on QUIC streams or WebSocket. Audio I/O uses `cpal`; desktop state is exposed to QML through CXX-Qt. SQLite stores chat history with encrypted message fields.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the architecture diagram and [agents.md](agents.md) for implementation invariants.

## Modular Framework

Peers exchange capability descriptors after handshake. Compatibility requires the same capability ID and major version. A descriptor carries its channel kind, parameters, auth tier (`public`, `room-member`, or `trusted-peer`), and experimental flag.

`FeatureRegistry` registers descriptors and binds `FeatureModule` implementations. The runtime applies auth and quota checks before module dispatch. Bespoke namespaces require user consent on the client. Native module loading checks signatures against configured trusted signer keys; it does not isolate module code from the host process.

### Built-in Capabilities

| Capability | Purpose |
|---|---|
| `core.chat.v1`, `core.file.v1` | Direct chat and files |
| `core.audio.opus` | Direct voice |
| `core.video.v1`, `core.audio.content.v1` | Direct video and shared audio |
| `core.devices.v1` | Device routing support; not a completed pairing/sync product |
| `room.chat.v1`, `room.file.v1` | Room chat and requested file delivery |
| `room.audio.sfu`, `room.video.sfu`, `room.audio.content.sfu` | Room media |
| `web.host.app.v1` | In-app portal requests |
| `game.relay.v1` | Opaque game-session datagram relay |
| `transport.quic.*` | Transport framing and QUIC capability descriptors |

The full descriptors are in [wellknown.rs](rust/doubleslash-features/src/wellknown.rs). Media modules advertise capabilities and quotas while media bytes use dedicated datagram paths. The generic channel API also exposes streams and datagrams; dynamic channel tags are allocated from `0x10`–`0xEF`.

Auth and quotas apply to inbound and outbound traffic. Bulk room-file payload forwarding has a separate paced, bounded path so quota drops do not strand ordered transfers; signatures, freshness, frame limits, and queue limits still apply.

For module authors, [examples.rs](rust/doubleslash-features/src/examples.rs) contains an opt-in matchmaker example. Portal authors can use [web-sdk/doubleslash.mjs](web-sdk/doubleslash.mjs); game payload confidentiality beyond the authenticated transport is the application's responsibility.

## Security & Identity

- Long-term Ed25519 identities authenticate invites and signaling. Invite handshakes use ephemeral X25519, HKDF, and AES-GCM with transcript binding and expiry checks.
- Post-handshake signed signaling has timestamp freshness checks and per-sender replay deduplication. Real-time SFU audio and receiver-idempotent file chunk/completion messages are exempt from deduplication, but still require valid signatures and fresh timestamps.
- Direct application signaling is session-encrypted. Room chat, file chunks, and media are sealed before reaching the supernode; group-key installation checks the elected keyer. Room names, membership, traffic volumes, and file-offer metadata remain visible to the relay.
- Peer build-attestation messages carry signed claims and release metadata. They are not proof that a remote process is executing unmodified code.

See the [threat model](docs/THREAT_MODEL.md) for trust boundaries and limitations.

## Updates

The desktop client checks GitHub Releases at startup and hourly when `update_check_enabled` is on (the default). Turning it off prevents automatic checks. Applying an update runs the bundled installer; application binaries are not distributed peer-to-peer. Android does not use this desktop update checker.

Installer verification is channel-dependent:

- Published `.sha256` sidecars are checked when available.
- For stable releases, a supplied `releases_manifest.json` must have a valid project Ed25519 signature and contain the downloaded archive hash for that version. The current installer permits a stable release without a manifest; manifest verification is not unconditional.
- Nightly installation requires a manifest and checks its platform archive hash, but accepts that manifest without an Ed25519 signature.

The implementation is in [installer main.rs](rust/doubleslash-installer/src/main.rs), [gui.rs](rust/doubleslash-installer/src/gui.rs), and [release_manifest.rs](rust/doubleslash-installer/src/release_manifest.rs).

## Settings Reference

These are selected **desktop** defaults from [SettingsModel](rust/doubleslash-client/src/ui/settings_model.rs). Android has separate UI preferences.

| Setting key | Default | Meaning |
|---|---|---|
| `update_check_enabled` | `true` | Automatic GitHub update checks |
| `direct_p2p_enabled` | `true` | Accept direct QUIC sessions |
| `direct_p2p_port` | `61045` | Preferred direct UDP listener port |
| `upnp_enabled` | `true` | Router port mapping |
| `relay_port` | `0` | Automatic local relay port |
| `relay_allow_gated` / `relay_auto_renew` | `true` / `true` | Gated relay access and ticket renewal |
| `auto_connect` | `false` | Startup reconnection preference |
| `push_to_talk` / `voice_activation` | `false` / `false` | Optional transmission modes |
| `ptt_key` | `space` | Push-to-talk binding |
| `noise_suppression` / `noise_strength` | `true` / `moderate` | Microphone noise suppression |
| `jitter_buffer_depth` | `3` | Configured voice buffer depth, in frames |
| `audio_input_device` / `audio_output_device` | empty | System default audio devices |
| `video_enabled` / `video_input_device` | `false` / empty | Sharing off; no saved source |
| `video_quality` / `video_codec` | `balanced` / `auto` | Quality preset and codec preference |
| `video_adaptive_bitrate` | `true` | Lower bitrate in response to loss |
| `video_keyframe_secs` | `4` | Keyframe interval |
| `content_audio_mode` | `auto` | Follow the shared source; `system` shares machine audio, `off` disables it |
| `video_overlays_json` | `[]` | No picture-in-picture overlays |
| `attestation_policy` | `warn` | Peer build-attestation policy (`off`, `warn`, `strict`) |
| `ollama_enabled` | `false` | Optional AI assistant |
| `ollama_base_url` | `http://127.0.0.1:11434` | Ollama endpoint |
| `youtube_preview_enabled` | `true` | Local link-preview cards; playback contacts the host |
| `debug_logging` | `false` | Verbose diagnostics |

## Data and Files

Desktop profile resolution uses `DOUBLESLASH_KEY_DIR` / `DOUBLESLASH_HOME`, otherwise `~/.doubleslash`. Restored profiles are stored below the collection root in `profiles/<uuid>/`, selected by `active-profile`. Android uses app-private storage.

| Profile file | Purpose |
|---|---|
| `identity.dat` | Encrypted identity |
| `peers.dat` | Encrypted trusted peers and block state |
| `chat_history.db` | SQLite history with encrypted message fields |
| `my_rooms.dat` | Encrypted room definitions and sidebar hide state |
| `settings.json` | Desktop preferences |
| `device-key.dat` / `device-trust.db` | Local endpoint key / accepted device registries, where present |
| `logs/doubleslash-client.log` | Desktop session log, truncated at startup |

Desktop received files normally go to Downloads. Android keeps received files in app storage until exported through the document picker. Optional OS keyring/Android Keystore integration caches local unlock material.

### Devices & Backups

Use **Settings → Identity → Devices & Backups** on desktop or **Settings → Devices & Backups** on Android to export a verified encrypted `.dbackup`. It includes identity recovery material, peers, rooms, history, preferences, and optionally available attachments.

Restore from the unlock screen using the backup password and choose a new local passphrase. The original passphrase, keyfile, and keyring are not required. Restore creates a separate profile and preserves existing profiles; local device keys and native plugin binaries are not copied. See [Devices and backups](docs/DEVICES_AND_BACKUPS.md).

Keep the backup and its password separately. There is no account-recovery service. An `identity.dat` copy alone still needs its original unlock credentials and does not include history or attachments.

Supernodes separately persist their identity, peer records, reusable invite, endpoint mailbox, and operator configuration. Room hosting remains ephemeral; room definitions and chat history belong to clients.

## Privacy Policy

[PRIVACY.md](PRIVACY.md) contains the full policy; [TERMS.md](TERMS.md) covers user-generated content. Android requires terms acceptance before the home screen and provides an identifier-based report through the system share sheet.

DoubleSlash has no telemetry or central account/message store. Network activity includes peer/supernode connections, desktop update requests to GitHub, optional UPnP requests to the local router, user-opened video embeds, and requests to the configured Ollama endpoint. Disable these optional integrations in Settings where provided. Android does not run the desktop UPnP mapper or update checker.

Capture starts through call, share, or preview controls. Local preview stays on the device; shared media goes to call participants. Android uses a foreground service for active calls, so leaving the app or turning off the screen is not a hang-up action; stop the call or use Disconnect. DoubleSlash does not record calls to disk.

**A whole-screen share includes notifications and anything that appears over it. Per-application audio capture requires Windows build 20348 or later; on older builds it falls back to whole-machine audio.** See [capture disclosures](PRIVACY.md#camera-screen-and-shared-audio-capture).

## Troubleshooting

- **Cannot connect:** confirm the invite handshake completed, check listener/firewall reachability, and try a common trusted supernode if direct connections fail.
- **No voice:** check the selected audio devices, microphone permission, mute, and push-to-talk/voice-activation settings.
- **Room video missing:** check codec compatibility and the QUIC relay path. WebSocket-only room connections do not carry video.
- **Supernode unreachable:** check its configured UDP/TCP listeners and `supernode_host`. Keep its advertised address and signaling port stable.
- **Diagnostics:** desktop logs are in the active profile's `logs/doubleslash-client.log`. Enable verbose logging in Settings or set `RUST_LOG`; an explicit environment filter takes precedence. Copy the log before restarting, since startup truncates it. Android diagnostics use Logcat.

## Developer Guide

### Prerequisites

- Rust stable and a native C/C++ toolchain (MSVC on Windows).
- CMake and Perl for the vendored codecs. With CMake 4, set `CMAKE_POLICY_VERSION_MINIMUM=3.5` when invoking Cargo from the repository root (workspace-local Cargo configs set it when building from their directories). Initialize submodules:

  ```sh
  git submodule update --init --recursive
  ```

- Fetch the Opus DNN weights before building (DRED/OSCE are enabled by default):

  ```powershell
  powershell -ExecutionPolicy Bypass -File scripts/fetch_opus_weights.ps1
  ```

  Or on Linux/macOS:

  ```sh
  bash scripts/fetch_opus_weights.sh
  ```

- Desktop UI: Qt 6 with Quick/QML; set `QMAKE` or `CMAKE_PREFIX_PATH`. Add Qt WebEngine for the `webengine` feature. Linux builds also need ALSA, D-Bus, and libclang development packages; see the [build script](build_linux.sh) for its package list.
- Android: SDK, NDK, JDK, `cargo-ndk`, and the Rust Android target; follow [docs/ANDROID.md](docs/ANDROID.md).

### Building from Source

Run from the repository root:

```sh
# Desktop; omit ,webengine if Qt WebEngine is unavailable
cargo build --manifest-path rust/doubleslash-client/Cargo.toml --features qt-ui,webengine

# Server and installer
cargo build --manifest-path rust/Cargo.toml --release -p doubleslash-supernode -p doubleslash-installer
```

For Windows debug UI builds, put the matching Qt `bin` directory on `PATH` and run `rust\doubleslash-client\target\debug\doubleslash-client.exe`. On Linux/macOS the corresponding binary has no `.exe` suffix.

Packaging entry points:

| Target | Command |
|---|---|
| Windows desktop | `.\build_win64.ps1` |
| Linux desktop | `bash build_linux.sh` |
| macOS desktop | `bash build_macos.sh` |
| Windows supernode | `.\scripts\build_supernode.ps1` |
| Linux/macOS supernode | `bash scripts/build_supernode.sh` |
| Android debug APK | From `android/`: `./gradlew assembleDebug` (`.\gradlew.bat assembleDebug` on Windows) |

Windows desktop packaging writes `dist/DoubleSlash/` and a versioned `.7z`. Set `QT_DIR` if the script cannot find Qt. `run_client.bat` currently launches the packaged `dist/DoubleSlash/DoubleSlash.exe` with the repository's `.clientA` profile; it is not a debug-build launcher or a general profile selector.

### Two-Client Local Testing

Use distinct profile directories and identities. Launch the binary directly so the helper does not override the profile. In separate PowerShell terminals, from the repository root:

```powershell
# Terminal A
$env:DOUBLESLASH_HOME = "$PWD\.clientA"
$env:DOUBLESLASH_KEY_DIR = $env:DOUBLESLASH_HOME
.\dist\DoubleSlash\DoubleSlash.exe
```

```powershell
# Terminal B
$env:DOUBLESLASH_HOME = "$PWD\.clientB"
$env:DOUBLESLASH_KEY_DIR = $env:DOUBLESLASH_HOME
.\dist\DoubleSlash\DoubleSlash.exe
```

### Run Tests

There are separate Cargo workspaces for the outer crates, client, Android bridge, and supernode manager. The client is a library plus a desktop binary; headless tests do not require Qt.

```sh
cargo test --manifest-path rust/Cargo.toml --workspace
cargo test --manifest-path rust/doubleslash-client/Cargo.toml --no-default-features
cargo test --manifest-path rust/doubleslash-supernode-manager/Cargo.toml --workspace
```

Use `cargo test … -- --list` for current test inventories. Android validation commands are in its [guide](docs/ANDROID.md). Automated tests do not replace two-client and physical-device media checks.

Run `cargo fmt --all -- --check` and targeted `cargo clippy … -- -D warnings` in affected workspaces. [scripts/ci_local.ps1](scripts/ci_local.ps1) and [scripts/ci_local.sh](scripts/ci_local.sh) provide broader checks. [Coverage scripts](scripts/coverage.ps1) produce LLVM line/region reports; the shell counterpart is [coverage.sh](scripts/coverage.sh).

## Code Signing Policy

The [release workflow](.github/workflows/release.yml) supports Windows SignPath signing, macOS Developer ID signing/notarization, and GitHub artifact attestations when configured. Platform signatures depend on the release configuration; their presence should not be inferred from the version number.

The project Ed25519 release-manifest key is separate from platform code-signing certificates. Its public key is in [keys/release-signer-public.pem](keys/release-signer-public.pem) and [installer release_manifest.rs](rust/doubleslash-installer/src/release_manifest.rs). Verification behavior, including nightly exceptions, is described under [Updates](#updates).

### Team Roles

| Role | Team |
|---|---|
| Authors | [doubleslash-authors](https://github.com/orgs/DoubleSlashSpace/teams/doubleslash-authors) |
| Reviewers | [doubleslash-reviewers](https://github.com/orgs/DoubleSlashSpace/teams/doubleslash-reviewers) |
| Release signing approvers | [doubleslash-approvers](https://github.com/orgs/DoubleSlashSpace/teams/doubleslash-approvers) |

The in-tree manifest helper can generate, sign, and verify release manifests. From the repository root:

```sh
cargo run --manifest-path rust/Cargo.toml -p doubleslash-installer --bin sign-release-manifest -- --help
```

## Release Notes

Version-specific notes are published with [GitHub releases](https://github.com/DoubleSlashSpace/DoubleSlash/releases). Open work and acceptance gaps are tracked in [backlog.md](backlog.md); this README describes the implementation rather than repeating a milestone changelog.

## License

[MIT](LICENSE)
