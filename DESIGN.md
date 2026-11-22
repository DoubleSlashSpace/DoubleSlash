# DoubleSlash design system

The desktop Qt/QML and Android Compose clients share navigation, palette/skin
format and room/call semantics. Platform-native controls and window behavior
still differ. This guide describes implemented patterns and design rules;
it is not an accessibility or visual acceptance report.

Source owners:

- [Theme.qml](rust/doubleslash-client/qml/Theme.qml): desktop palette, sizing and motion tokens.
- [Android Theme](android/app/src/main/java/com/doubleslash/client/ui/Theme.kt) and
  [Skin](android/app/src/main/java/com/doubleslash/client/ui/Skin.kt): Compose palette and portable skins.
- [Client feature reference](docs/CLIENT_FEATURES.md): behavior and platform differences.
- [Backlog](backlog.md): unfinished UI and hardware acceptance.

## Principles

Keep useful information compact, with clear hierarchy and restrained motion.
Use shared tokens, semantic labels and reusable controls. Keep transport and
trust decisions in the Rust model; the UI consumes their state. A visible control
must reflect the operation's actual availability and failure result.

## Palette and typography

The default base is dark; light and system modes are supported. Settings >
Appearance offers DoubleSlash, Midnight, Slate, Forest, High contrast and Paper
skins, per-color edits and portable JSON Copy/Paste across clients. A skin has
version, name, base and color overrides; text scaling and date format are separate
preferences. Desktop icon packs are a folder of the packaged SVGs, or a
PNG of the same name (`icon_pack_dir`, Settings › General › Icon pack), not
part of a skin.
Android uses the app palette rather than wallpaper-derived colors.

| Token | Default dark value | Use |
|---|---|---|
| `bg0` | `#111214` | Window chrome and deepest layer |
| `bg1` | `#1E1F22` | Panels and sidebar |
| `bg2` | `#2B2D31` | Cards and inputs |
| `bg3` | `#383A40` | Hover and subtle surfaces |
| `text` / `muted` | `#DCDDDE` / `#A6A9B0` | Primary and secondary text |
| `textInv` | `#FFFFFF` | Text on colored surfaces |
| `accent` | `#5865F2` | Selection and focus |
| `online` | `#3BA55D` | Connected state and affirmative status |
| `warn` | `#FAA61A` | Warning or degraded state |
| `danger` | `#FF2B40` | Errors, destructive controls and brand accents |

Light colors and named skin overrides come from the implementations above;
do not duplicate another palette table. Brand use of red is intentional, so
labels and icons must distinguish it from an error. There is no `success`
color token in the desktop theme; use the existing semantic token.

At zero text adjustment, desktop title/body/caption sizes are 15/13/11 px.
`font_scale_percent` ranges from -50 to +200; consume the scaled tokens
instead of duplicating those literals. `icon_scale_percent` uses that same
range for icon glyphs (`Theme.iconPx`); it is not a skin token. Desktop spacing tokens are 4, 8, 12,
16 and 24 px. Angular components use zero small/medium/large radius; circular
badges use `radiusPill`. Controls derive height from font size with a 32 px
minimum and `touchTarget` has a 44 px minimum.

Motion tokens are `animMicro` (80 ms), `animFast` (160 ms),
`animNormal` (250 ms) and `animSlow` (300 ms). Use the relevant
`Theme.anim*` token for QML behaviors. Avoid timers and animations that
rebuild lists or add work to media paths.

## Navigation and rooms

Peers | Rooms sits beside the D:// logo at the top. The logo/avatar opens
Settings on Identity. Contextual add/join actions follow the selected list;
there is no bottom tab bar. Our own row is pinned above trusted peers and opens
Message myself, a live text conversation with our other connected devices.
Its presence count is independent of contact presence.

Each supernode/cluster heads its own rooms with a chevron, connected avatar ring
and local name/operator title/short ID. A node can be folded, reordered and
renamed locally. Unrelated nodes keep separate room rows even when room IDs
match. Rooms have Voice and Text-only leaves before sub-rooms. A member's
inline actions include trust invite/message, video watch and local audio controls
as permitted by membership and current voice state.

Desktop uses `RoomTree.qml` and `RoomTreeMember.qml`; Android uses
`ui/RoomTree.kt` and opens the same leaves in its Members sheet. A live
`VoiceDock` stays at the foot of the sidebar/screen, independent of room
selection. Leave ends voice and keeps text selection. Do not restore a separate
room-member panel based on the older VoiceRail layout.

Hide Room is a local tombstone. Desktop Hide also leaves voice/unsubscribes;
Android Hide only hides the row. Remove Room removes the stored room across own
devices and asks whether to delete this device's history too. It does not delete
the room for other people. Both confirmations use "Are you sure?". The built-in
`default` room has Hide but no Remove. On hidden rows, keep the rest of the
menu visible and disabled while Show in list and Remove remain enabled.

## Reusable components

| Component | Pattern |
|---|---|
| `TitleBar` | Desktop drag/maximize/window controls; close has destructive-colored hover |
| `StyledButton` / `StyledTextField` | Shared compact controls, aligned icons/labels, focus/hover/disabled states and tooltips; the field's placeholder stays inside it rather than floating over a label |
| `StyledButtonBox` / `SquareCheckBox` | Square dialog footer (accept button filled) and checkbox, in place of Material's pill buttons and rounded indicator |
| `Avatar` / `SupernodeAvatar` | Identity-derived artwork generated locally; speaking/connection rings consume model state |
| `ConnectionStatsChip` / `StatsPanel` | Per-peer or node path/latency and detailed diagnostics |
| `ChatPanel` / `RoomPanel` | Local history, transfer cards, visible read state, optional inline link playback |
| `VideoRegion` / tiles / pop-out | Opt-in watched video, local preview and explicit camera-off versus stalled state |
| `TrustInviteDialog` | Sender identity, room, explanation of trust, Not now and Accept; receiving does not trust automatically |
| `FilePreviewPanel` | Lazy WebEngine preview strip above the chat, below watched video; close and open-in-system-app actions |
| `HostSupernodeWizard` / Your servers | Fingerprint/responsibility review, progress, transient credentials and explicit partial failure |

Local file preview supports images, documents/text, PDF and playable audio/video
through WebEngine; unsupported files show a fallback. Builds without WebEngine
open the system application. The preview is part of the chat layout, not a
full-window modal that hides the conversation.

`DoubleSlashWebView.qml` controls top-level navigation using allowed domains,
portal mode and explicit general-browsing mode. The native scheme handler in
`ui/scheme.cpp` installs portal handlers on the default profiles and injects
the limited `window.doubleslash` game/channel bridge for portal pages. Do not
claim "no native bridge" or infer a network-subresource sandbox from the QML
navigation allowlist. Keep new privileged functions out of arbitrary pages.

## Interaction and accessibility rules

- Consume `Theme.*` colors, fonts, spacing and radii. Follow background
  layers in hierarchy; keep contrast visible in both base palettes and skins.
- Use shared controls with keyboard activation, visible focus, accessible names
  and tooltips for icon actions. Do not convey trust, mute or failure only by color.
- Keep live call controls reachable while browsing other screens. Avoid moving
  or resetting per-peer controls as levels/presence update.
- Use local avatars and the bundled SVG icon system. Do not describe avatar
  generation as a server operation or assume an external Material Icons font.
- Verify narrow windows, large text, high DPI, long names, hidden/disabled rows
  and touch use. Accessibility targets are requirements to validate, not a
  certification claim.

## Verification

On Windows with Qt installed:

```powershell
.\scripts\test_qml_controls.ps1
.\scripts\test_backup_wizard.ps1
```

Those scripts stage production QML with fixtures/mocked backends. They cover
specific controls/wizard states, not full two-client or hardware acceptance.

`.\scripts\test_ui_audit.ps1 [-Build] [-Matrix full]` runs the built client
itself on a throwaway profile. It walks the Peers and Rooms lists, sample
conversations, every Settings section and the dialogs and wizards in dark,
light, large-text and minimum-window variants (`full` adds the other text
sizes and every skin). On each screen it checks every visible item for text
overflow, clipping, overlap, contrast, items escaping a dialog, small or
unlabeled controls and the angular rule. It writes an annotated `report.html`
under `rust/target/ui-audit/` and fails on errors. The script's help lists the
rules, the allowlist format and what the tour cannot reach.
UI changes also need a Qt-enabled build and the relevant Android checks.
Record packaged-client results in [acceptance](docs/ACCEPTANCE.md) or the owning
guide and keep remaining work in [backlog.md](backlog.md).
