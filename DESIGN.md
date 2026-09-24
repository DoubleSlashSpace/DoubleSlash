---
name: DoubleSlash Design System
version: 1.0.0
theme: DoubleSlash Dark (Dracula/Discord-inspired)
primary_palette: Dark (with full light mode support)
framework: Qt 6 + QML (Rust / CXX-Qt)
status: Active
last_updated: 2026-06-26
---

# DoubleSlash Design System

## Overview
DoubleSlash is a high-information-density, privacy-focused P2P application. The UI emphasizes clarity, speed, and minimal distraction through:

- A layered dark-first aesthetic with subtle depth (no heavy shadows).
- Discord/Dracula-inspired color language.
- Strong typography hierarchy and consistent spacing.
- Fluid micro-animations on state changes.
- Full light mode support via `Theme.isDark`.

**Source of truth**: `rust/doubleslash-client/qml/Theme.qml` + this document.
All new QML must consume tokens from `Theme.*` — never hardcode colors, sizes, or radii.

**Design Principles**:
- **Density over whitespace** — maximize useful content.
- **Clarity first** — semantic colors and clear visual hierarchy.
- **Motion with purpose** — short, meaningful animations (≤ 300ms).
- **Consistency** — every component follows the token system.
- **Performance** — lightweight delegates, lazy loading, minimal JS.

## Tokens

### Colors

#### Background Stack (creates depth)
Use layers strictly in order for visual hierarchy.

**Dark Mode (default)**
| Token       | Hex       | Role                              |
|-------------|-----------|-----------------------------------|
| `bg0`       | `#111214` | Window chrome, title bar          |
| `bg1`       | `#1E1F22` | Primary panels, sidebars          |
| `bg2`       | `#2B2D31` | Cards, inputs, section headers    |
| `bg3`       | `#383A40` | Hover, dividers, subtle surfaces  |

**Light Mode**
`bg0 #F2F3F5` → `bg1 #FFFFFF` → `bg2 #E9EAEC` → `bg3 #D8D9DD`

#### Text
| Token       | Hex       | Role                                      |
|-------------|-----------|-------------------------------------------|
| `text`      | `#DCDDDE` | Primary body / labels                     |
| `muted`     | `#A6A9B0` | Secondary, captions, placeholders         |
| `textInv`   | `#FFFFFF` | Text on accent / semantic colored bg      |

Light-mode secondary text uses `#5B5F68`. Both secondary-text colors meet
4.5:1 contrast against every solid background layer (`bg0` through `bg3`).

#### Semantic
| Token       | Hex       | Role                                      |
|-------------|-----------|-------------------------------------------|
| `accent`    | `#5865F2` | Focus, selection, active states           |
| `online`    | `#3BA55D` | Live connections, success                 |
| `warn`      | `#FAA61A` | Relay / non-ideal states                  |
| `danger`    | `#FF2B40` | Errors, destructive actions, **brand**    |

### Typography
Uses system font (via `Material.Dark` / `SystemDefault`). No custom font loading unless decided later.

| Token              | Size   | Weight     | Role                          |
|--------------------|--------|------------|-------------------------------|
| `fontSizeTitle`    | 15px   | Medium     | Panel titles, peer names      |
| `fontSizeBody`     | 13px   | Regular    | Messages, main content        |
| `fontSizeCaption`  | 11px   | Regular    | Timestamps, badges, labels    |

**Rules**:
- Section headers: `uppercase`, `letterSpacing: 1.2`, `muted` color.
- Use `font.pixelSize` + `Theme.*` tokens.

### Spacing & Radius
| Token         | Value   | Usage                              |
|---------------|---------|------------------------------------|
| `spacingXs`   | 4px     | Icon gaps, tight elements          |
| `spacingSm`   | 8px     | Inner padding, icon+label          |
| `spacingMd`   | 12px    | Standard item padding              |
| `spacingLg`   | 16px    | Panel margins                      |
| `spacingXl`   | 24px    | Major section gaps                 |
| `radiusSm`    | 0px     | Angular inputs, chips, pills       |
| `radiusMd`    | 0px     | Angular buttons, cards, dialogs    |
| `radiusLg`    | 0px     | Angular panels / overlays          |
| `radiusPill`  | 999px   | Fully-rounded badges, status dots, circular icon buttons |

### Motion
| Token         | Value   | Usage                                              |
|---------------|---------|----------------------------------------------------|
| `animMicro`   | 80ms    | Live audio rings, hover color flips, micro state   |
| `animFast`    | 160ms   | Mute toggle, panel open/close, opacity fades       |
| `animNormal`  | 250ms   | List item bg, selection highlight                  |
| `animSlow`    | 300ms   | Session banner tint, accent bar color              |

All `Behavior` blocks must reference a `Theme.anim*` token — never a raw integer.

## Components

### Core Components (Document each with)
- Visual description + screenshot (add images here when possible)
- Key properties & states
- Usage example / code snippet

### TitleBar
44px frameless custom bar (`bg0`). Hosts drag-to-move, double-click-to-maximize, and three window-control buttons (minimize, maximize/restore, close) via an internal `TitleBarButton` component. Button hover fills: `bg3` for minimize/maximize, `danger` for close. Default slot between logo and buttons accepts arbitrary `contentChildren` via `Layout.*`.

The main window invite field and actions use `StyledTextField` and
`StyledButton`: aligned 32px controls, 20px action icons, accessible names,
keyboard activation, and tooltips on hover or keyboard focus. Peers/Rooms
tabs use body-size labels and the shared theme accent.

### Shared Controls
`StyledButton` keeps its background at the full control height without Material
insets or Material's 14px vertical padding, so icon and label stay centered in
the 32px control. Labels elide within constrained widths; icon-only actions stay centered.
Neutral actions use the selected fill when pressed. Keyboard focus has a 2px
text-color border, visible on both neutral and filled buttons.

`StyledTextField` uses a `bg2` surface, a divider border, standard horizontal
padding, and a 2px accent focus border. Hover strengthens the border; disabled
fields use `bg1` and muted text. Text selection uses the theme accent.

Run `scripts/test_qml_controls.ps1` on Windows with Qt installed (override
`-QtBin` for another installation). It stages production components into a
temporary QML module and tests contrast, constrained labels, icon alignment,
pressed feedback, keyboard focus/activation, and disabled field states without
loading a client identity or connecting to peers.

### SessionBanner
32px status strip below TitleBar. Left edge: 3px accent bar tinted by connection mode color. Row: 7px circular status dot + mode label (`connectionModeColor`) + optional `bannerText`. Background is `Qt.tint(bg1, connectionModeTint(mode))`. All color transitions use `animSlow`.

### PeerList / SidebarItem
**PeerList** — `bg1` panel. Section headers (Online / Offline) in `muted`, uppercase, `letterSpacing: 1.2`. Peer rows are 56px tall; selected state uses `selectedFill()` + 3px left accent bar. Avatar ring color: `danger` (blocked) → `online` (connected) → avatar tint (offline). Unread badge: `danger` fill, `radiusPill`, caption text. Right-click context menu: Start Call / Copy Peer ID / Remove Peer / Block or Unblock Peer / Clear Chat History.

**SidebarItem** — 44px `ItemDelegate`. Selected: `selectedFill()` bg + 3px `accent` left bar. Badge: `accent` fill, `radiusPill`.

### ChatPanel
Message list with bubbles, typing indicator, date separators, file-transfer chip UI (progress bar), Ollama streaming, and paginated history. Includes a stats overlay (`StatsPanel` + `ConnectionStatsChip`) anchored to the top-right of the message area.

### CallPanel
240×80px floating `bg2` rectangle with `accent` border. Contains: circular 10px state indicator (`online` / `warn` / `danger`), status text, mute toggle (mic icon colored by `online`/`danger`), end-call button (danger `x-circle` icon). Font uses `fontSizeBody`. Margins via `spacingMd`.

### Avatar
SVG identicon generated by `backend.avatarSvg(peerId, configJson)`. Default tint for unknown peers is `Theme.muted`. Optional status ring controlled by `showRing`, `ringColor`, `speaking`, and `audioLevel`:
- Static ring: `showRing: true`, fixed `ringColor`, ring width ≈ 17% of `size`.
- Speaking ring: `speaking: true` hides Avatar's own border and reserves a ring width ≈ 22% of `size`; the activity ring itself is drawn by the parent (see `MemberRow`).

Ring color convention in PeerList: `danger` = blocked, `online` = connected, avatar tint = offline.

---

### ConnectionStatsChip
Compact inline chip in the chat header showing live RTT, relay status, and a quality-colored indicator dot. Color thresholds: ≤150ms+<1.5% loss = `online`, ≤300ms+<4% = `warn`, else `danger`. Clicking expands `StatsPanel`. Hidden when no stats data is available (`hasData: false`).

### StatsPanel
220px overlay showing RTT, packet loss, jitter, bandwidth, and a quality tier label (Excellent / Good / Fair / Poor). Includes a 30-sample RTT sparkline canvas using `rttSparklineColor()` for the line color. Background: `overlayScrim` at 82% alpha. Dismisses on click-away.

### VoiceRail
The collapsible right-hand column: the room's one member list plus the voice controls. Its width animates 0↔220px via `animFast + InOutQuad`. It is shown while voice is live anywhere, and while a room is open with its members toggle on. While a room is open it lists the whole room (`roomView`); otherwise it lists the live voice session. Structure:
- **Header** (52px, `bg3`): In a room it shows "Members · N", and "You are not in voice" or "Your voice is in …" when voice is elsewhere. Outside a room it shows the supernode Avatar, the room or peer name, and the connection mode pill.
- **Member list**: A `ListView` of `MemberRow`s. In a room it has two sections, "In voice" then "Text only". It shows a connecting spinner until someone appears.
- **Trust-invite notice** (`bg3`, `warn` text): Why the last invite was not sent. It clears itself.
- **Duration counter** (24px, `bg3`): A `mm:ss` or `h:mm:ss` monospaced timer, shown only while voice is live.
- **Controls bar** (52px, `bg3`): Shown only while voice is live. A mute toggle (36px circular, `bg2`/`danger` fill, `animFast` color), share, and End/Leave (36px circular, `danger`). All buttons use `radiusPill`.

One shared `Menu` serves every row. It offers Watch video / Stop watching and Pop out video. Those, like Mute for me and Volume…, are enabled only for members of the voice session we are in. It also offers Message (trusted peers) or Invite to trusted peers (everyone else), and Copy Peer ID.

### MemberRow
One person in the rail, 46px tall. It has a 28px `Avatar`, wrapped by a property-bound level ring while the member is in voice. The ring uses a heat-map color (cool teal → warm green → hot yellow), and its width and opacity animate at `animMicro`. Beside the avatar are the name (bold while speaking) and a status line: "In voice · muted · muted for you", "Text only", or the trust-invite state in `accent`. Text-only rows are drawn at 60% opacity. Badges:
- a 14px `danger` mic-off badge for the member's own mute;
- a `danger` ring for "muted for me";
- a 22px camera badge while they stream, `accent`-filled while watched. Clicking the badge toggles watching; clicking anywhere else opens the menu.

### TrustInviteDialog
A modal card over a scrim, the same shape as `IncomingCallDialog`: a room member offers to become trusted peers. It shows their avatar, their self-chosen name, the room, their full id, and what trust allows. Buttons are Not now and Accept (`success`). Offers queue behind the first, and one left unanswered for 14 minutes is dropped.

### FilePreviewPanel
Inline file preview using `DoubleSlashWebView`. Supports: images, PDF, HTML, text/code, video (HTML5 `<video>`), audio (HTML5 `<audio>`). Navigation restricted to `file://` and `data:` URIs — no outbound network. Shows "cannot preview" message for unsupported types.

### DoubleSlashWebView
Shared secure `QtWebEngine` wrapper. Always off-the-record (no persistent cookies, cache, localStorage, or history). Navigation whitelist: only hosts matching `allowedDomains` suffixes are allowed; `file://` and `data:` are always permitted. `allowAll: true` bypasses the whitelist (browser panel). `allowPortal: true` permits `doubleslash://` URLs for supernode portal pages. No `QWebChannel` bridge — zero access to Rust/AppBridge peer data.

## States & Interactions

- **Hover**: `bg3` fill or slight brightness shift.
- **Selected / Active**: `accent` 15% fill + 3px left accent bar with angular ends.
- **Focus**: `accent` border.
- **Disabled**: 50% opacity + `muted` text.
- **Transitions**: Use `ColorAnimation { duration: 250 }` or `NumberAnimation` for all state changes.
- **Right-click**: Consistent context menus with Qt `Menu`.

## Icons & Assets
- Prefer **Material Icons** (via `MaterialIcon` or SVG) for consistency.
- Custom SVGs only for brand (logo, avatars).
- All icons: 20–24px, use `Theme.text` or semantic colors.
- Avatars: Generated server-side via `backend.avatarSvg(...)`.

## Theming & Modes
- Toggle: `Theme.isDark` (reactive property).
- All colors defined as properties in `Theme.qml`.
- Light mode is fully supported but secondary — test both regularly.

## Do's and Don'ts (Rules)

**Do**:
- Always use `Theme.*` tokens.
- Follow the background layer order (`bg0` → `bg1` → `bg2` → `bg3`).
- Keep animations short and purposeful.
- Use Layouts + anchors for responsiveness.
- Test on multiple DPIs / window sizes.

**Don't**:
- Hardcode hex values, pixel sizes, or colors.
- Use `danger` for non-destructive elements (it's the brand color).
- Overuse `accent` — reserve for focus/selection.
- Put heavy logic inside UI components (keep in Python/Rust models).

## Accessibility
- Minimum contrast ratios (WCAG AA).
- Keyboard navigation support.
- Screen reader friendly labels (`Accessible` role).
- Scalable fonts and touch targets (min 44px).

## Screenshots / Visual Inventory
*(Add gallery here as components mature)*

## Changelog
- 2026-09-23: Merge the room members panel into `VoiceRail`, giving a room one list with in-voice and text-only sections. Replace the `ParticipantWidget` tiles with `MemberRow`. Add `TrustInviteDialog`. Drop the rail's unused ring-history timers.
- 2026-08-10: Remove `TalkingRing` — the component was never instantiated and was absent from the QML module's file list, so it shipped in no build. `ParticipantWidget` draws the activity ring.
- 2026-06-26: Add `radiusPill` + `animMicro` tokens; sweep all hardcoded animation durations and badge radii to token references; document TitleBar buttons, Avatar ring behavior, and 7 new components (ConnectionStatsChip, StatsPanel, TalkingRing, VoiceRail, ParticipantWidget, FilePreviewPanel, DoubleSlashWebView)
- 2026-06-06: Initial design system document v1.0

---