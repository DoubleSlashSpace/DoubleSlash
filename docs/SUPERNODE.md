# DoubleSlash Supernode Operator Guide

This guide covers running a production or volunteer `doubleslash-supernode`.

The supernode provides optional transport assistance (QUIC relay, SFU rooms) and hosts opt-in feature modules including an **in-app portal** (`web.host.app.v1`). It is **never** an identity or trust authority — all trust comes from the invite + handshake between peers. There is no public WebTransport / HTTPS game surface.

## Quick Start

```bash
# Build from the outer Rust workspace
cd rust
cargo build -p doubleslash-supernode --release

# Run with defaults (data in $HOME/.doubleslash)
./target/release/doubleslash-supernode
```

### Pre-built binaries (GitHub Releases)

Official tagged releases and the rolling `nightly` prerelease ship standalone supernode packages (each with a `.sha256` sidecar). These are the easiest path for VPS and bare-metal hosts — no Rust toolchain required on the server.

| Platform | Tagged release asset | Nightly asset |
|---|---|---|
| Linux x86_64 | `doubleslash-supernode-<version>-linux-x86_64.tar.gz` | `doubleslash-supernode-nightly-linux-x86_64.tar.gz` |
| Linux ARM64 (`aarch64`) | `doubleslash-supernode-<version>-linux-aarch64.tar.gz` | `doubleslash-supernode-nightly-linux-aarch64.tar.gz` |
| Windows x86_64 | `doubleslash-supernode-<version>-win64.zip` | `doubleslash-supernode-nightly-win64.zip` |

`<version>` is the product stamp `{major}.{minor}.{build}.0`, the same string the binary reports.

**Linux x86_64** (typical VPS / cloud VM):

```bash
tar -xzf doubleslash-supernode-<version>-linux-x86_64.tar.gz
sudo install -m 755 doubleslash-supernode-<version>-linux-x86_64/doubleslash-supernode /usr/local/bin/
```

**Linux ARM64** (Raspberry Pi, ARM VPS):

```bash
tar -xzf doubleslash-supernode-<version>-linux-aarch64.tar.gz
sudo install -m 755 doubleslash-supernode-<version>-linux-aarch64/doubleslash-supernode /usr/local/bin/
```

**Windows x86_64**:

```powershell
Expand-Archive doubleslash-supernode-<version>-win64.zip -DestinationPath .
# Run: .\doubleslash-supernode-<version>-win64\doubleslash-supernode.exe
```

### Build and package locally

On Linux or macOS, `scripts/build_supernode.sh` detects the host platform and emits a `.tar.gz` under `dist/`:

```bash
DOUBLESLASH_RELEASE=1 ./scripts/build_supernode.sh
# e.g. dist/doubleslash-supernode-1.0.467.0-linux-x86_64.tar.gz
```

On Windows, use the companion script (`.zip` output):

```powershell
$env:DOUBLESLASH_RELEASE = '1'
.\scripts\build_supernode.ps1
# e.g. dist\doubleslash-supernode-1.0.467.0-win64.zip
```

Supported local package suffixes: `linux-x86_64`, `linux-aarch64`, `macos-arm64`, `macos-x86_64` (shell script), and `win64` (PowerShell script).

CI validates packaging on all three release targets (`test-supernode-linux-x86_64`, `test-linux-arm64`, `test-supernode-windows` in `.github/workflows/ci.yml`).

Hosted feature declarations are read from `<data_dir>/supernode.toml` (see below). If the file is missing, a full first-party default manifest is used. Built-in first-party descriptors are always present in the registry for quota and relay accounting.

## Configuration (supernode.toml)

Create `<data_dir>/supernode.toml`. The default data dir is `$DOUBLESLASH_HOME` when set, otherwise `$HOME/.doubleslash`.

Example:

```toml
schema_version = 1

# Basic network
listen_addr = "0.0.0.0:3478"          # QUIC relay
ws_listen_addr = "0.0.0.0:34935"      # WebSocket signaling

# Feature manifest (recommended)
[[feature]]
id = "core.chat.v1"
enabled = true

[[feature]]
id = "room.audio.sfu"
enabled = true

[[feature]]
id = "room.file.v1"
enabled = true

[[feature]]
id = "web.host.app.v1"
enabled = true

[[feature]]
id = "game.relay.v1"
enabled = true

# Bespoke / third-party modules (x.*)
[[feature]]
id = "x.acme.matchmaker"
enabled = true
cdylib_manifest = "plugins/acme-matchmaker.toml"
```

See `rust/doubleslash-supernode/src/manifest.rs` for the full schema. The example above is the current starting point; the binary does not expose a manifest-printing CLI flag.

## Key Features & Hosting

### QUIC Relay
- Native peers connect via `QuicRelayClient` using Ed25519 mTLS + signed tickets.
- Tickets are issued with 1h TTL, 10min renewal window.
- Endpoint mailbox (`supernode_endpoints.json`) helps clients survive restarts (24h TTL).

### SFU Rooms (`room.audio.sfu` + `room.chat.v1` + `room.file.v1`)
- Up to 32 participants per room.
- Native desktop clients only (in-app portal games do not carry room voice/chat).
- Room file transfers use signed `SfuFile*` frames and are verified by recipients before saving. They are **advertised, then pulled**: `sfu_file_offer` carries metadata only, and chunks are sent only to members who answer with `sfu_file_request`. The supernode routes those chunks to the single peer named in the frame's `to` field and, as always, stores nothing — a member who accepts late is served by the original sender re-reading the file, not from any relay cache.
- Updated clients accept files up to 4 GiB (4,294,967,296 bytes); older clients still enforce their own smaller limit. Anything over 8 MiB is streamed from and to disk, so neither client holds the file in memory. This is a client limit; supernode quotas and queues do not increase.
- Offers remain available for one hour. An accepted transfer can continue past that deadline while it makes progress; an hour without progress expires it. Transfers are not resumed across client restarts.
- Because the sender holds the only copy, **deleting a file message revokes the share** (`sfu_file_revoke`): members who have not downloaded it yet can no longer obtain it. Members who already downloaded keep their copy.
- Room membership is enforced at the capability layer (`room-member` auth tier).
- Operators can restrict which room types peers may **create** via `room.audio.sfu` manifest params:
  - `allow_public_rooms` (default `false`) — when `false`, new public room materialization is rejected. The built-in **Public Voice/Chat Room** (`room_id = "default"`) is always present on SFU-enabled nodes.
  - `allow_private_rooms` (default `true`) — when `false`, new private room materialization is rejected.
  - Existing rooms may still be replayed/materialized by id; policy applies only to **new** room creation.
  - Denied creates return `sfu_room_created` with `denied: true` and a `reason` (`public_rooms_disabled` / `private_rooms_disabled`).

Example:

```toml
[[feature]]
id = "room.audio.sfu"
enabled = true
params = { allow_public_rooms = false, allow_private_rooms = true }
```

### In-app portal (`web.host.app.v1` + `game.relay.v1`)
- `web.host.app.v1` ships enabled by default: portal pages for the desktop client's embedded browser (`d://` scheme over QUIC bidi streams).
- `game.relay.v1`: opaque game-session datagram fan-out on the identity QUIC relay (fixed channel tag). Games open only from the native portal — **no public HTTP/WebTransport port, no TLS certs**.
- Static assets live in `<data_dir>/web/` and `<data_dir>/games/<slug>/` (seeded by the binary).

### Plugin / Bespoke Modules (`x.<vendor>.*`)
- Native cdylib plugins are loaded at startup from paths declared in the manifest.
- Each plugin must provide a manifest + signed binary (see loader in main.rs).
- First-party namespaces (`core.*`, `room.*`, `web.*`, `game.*`) bypass user consent prompts. `x.*` namespaces require explicit consent.

## Access Control

Set `access_mode` in `supernode.toml`, or use `supernode_access_mode` when the manifest omits it. Supported modes are:

- `open`
- `tos` (terms of service acceptance)
- `code` (also `access_code`)
- `ad` (also `timer`, the timer / ad-gate)

The manifest takes precedence. Unknown manifest modes and invalid manifest files stop startup. Code-gate credentials still come from `supernode_access_code`; configure that environment variable on the host.

Listener addresses in `listen_addr` and `ws_listen_addr` preserve both the IP and port, including loopback restrictions. Use numeric IP addresses and bracket IPv6, for example `[::1]:3478`. Invalid addresses stop startup. Without these fields, listeners use `0.0.0.0` and their environment-configured ports.

See `src/access.rs` for the trait and examples.

## Tickets & Endpoint Mailbox

- Relay tickets are Ed25519-signed by the supernode. `relay_host` is the IP in `supernode_host`.
- Optional `hostname` in `supernode.toml` (or `supernode_hostname`) is a DNS name signed into the invite beside that IP and advertised on `SupernodeInfo`. Clients dial the IP. If that connection fails, they resolve the name as `hostname_lookup` (or `supernode_hostname_lookup`) says. With `dns` they use DNS at the signaling port. With `dddns` the name is a [DDDNS](DDDNS.md) name in any zone, signed into the invite as `dddns://<name>`, and clients fetch `GET https://<name>/.well-known/doubleslash` for the current IP and port. Unset, it is `dddns` for names under `*.doubleslash.space` and `dns` otherwise. Clients dial the address they get back, and the node must sign the client's HELLO challenge before the client uses the connection. A failed lookup leaves them on the invite address. With a hostname set, an IP or signaling-port change keeps the same reusable invite id and rewrites the current IP into it. Without one, that change mints a new invite.
- In the desktop setup wizard, enter an optional **Hostname** and choose **Automatic**, **DNS** or **DDDNS**. For an existing managed server, open **Settings > Network > Your servers**, expand its supernode, then select **Set hostname** (or its current hostname) under that server. **Apply and restart** pushes the configuration over pinned SSH and restarts only that server; existing connections briefly disconnect. An empty hostname removes it. Cluster members retain their cached roster; if the cache is missing, run **Sync cluster** first. Use a registered name you control. For DDDNS, enter its masked update token and review the displayed HTTPS update URL, then choose **Apply and update DDDNS**. The hostname is applied over SSH first; the desktop then freshly checks the selected server's addresses and publishes them with its WebSocket port. The fixed update URL for `*.doubleslash.space` is `https://users.doubleslash.space/v1/update`; the form makes it read-only and the backend rejects overrides. Another provider implementing the same update API needs its explicit HTTPS update URL. The token is never saved to the inventory or installed on the supernode, is cleared from the form on submission or closure, and must be entered again for a retry. Applying the same name to another server immediately replaces the record. Both address families are sent explicitly: missing families and IPv6 on IPv4-only signaling are cleared, so a previous server's address cannot survive the move. Edits to the same DDDNS record take turns within this client. A failed publication is reported separately after successful server configuration; setup retains the new managed server and displays a warning directing you to Your servers to retry. Success requires a matching record returned by the service. Updates use HTTPS with no redirects, a 15-second request timeout and a bounded confirmation body; service errors never display arbitrary response bodies. Removing a hostname does not delete its DDDNS record. This is a client-initiated update on Apply/setup, with no continuous updater or token left on the old server. Provider lookup caches may retain the previous record for up to the protocol's 30-second cache lifetime.
- Each server in **Your servers** shows selectable **IPv4**, **IPv6**, **WebSocket signaling port (TCP)** and **Relay port (UDP)** readouts. Entering a hostname, or opening an existing hostname, checks IPv4/IPv6 route source addresses over pinned SSH and shows the result directly beneath the hostname field. **Refresh addresses** repeats the read-only check. Public route addresses replace older configured addresses; the configured public IP is retained when a route source is private (for example, a server behind NAT). Unchecked families say **Not checked**; a successful check that finds no public address says **No public address detected**. SSH errors leave previous results visible with an error. This checks the server's local routes, without resolving the DDDNS hostname or contacting an external IP service, and does not prove external reachability or discover an unknown NAT mapping. The WebSocket port comes from the managed configuration: the guided setup uses **34935**, while custom ports use the resolved manager inventory. Enter the available IPv4/IPv6 addresses and WebSocket signaling port in DDDNS. IPv6 is shown without socket brackets. For an existing IPv4 wildcard listener with a detected IPv6 address, the readout says Apply will enable dual stack. Setup and hostname Apply check IPv6 availability on the target: wildcard listeners use `[::]` with `IPV6_V6ONLY` explicitly disabled for both TCP signaling and UDP QUIC relay, or `0.0.0.0` when IPv6 is unavailable. An existing wildcard bind is upgraded on Apply; explicit interface and loopback binds are preserved. The supernode also falls back from an IPv6 wildcard to IPv4 when IPv6 is unavailable at startup, while port conflicts and permission errors remain fatal. These guarantees require the updated supernode binary. Provider and host firewalls still need to allow the signaling TCP and relay UDP ports in each address family. Unresolvable inventory leaves ports unavailable.
- Clients should renew tickets when `needs_renewal()` returns true.
- The endpoint mailbox allows clients to discover the current relay address after a supernode restart without re-onboarding.

## Hot Reload & Operations

- The supernode does **not** currently support hot-reload of the binary.
- For plugin updates, restart is required.
- Use a process supervisor (systemd, docker, etc.) for production.

Graceful shutdown is supported (closes QUIC endpoints cleanly).

## Security Notes

- The supernode only sees encrypted traffic and metadata it needs for routing (peer indices, room membership).
- All feature dispatch goes through `doubleslash-features` (auth tier + quota enforcement).
- Never trust the supernode for identity — only for transport assistance and opt-in hosting.

## Monitoring & Stats

The supernode exposes basic stats via:
- `web.host.app.v1` portal endpoints (`/health`, `/api/stats`, `/api/metrics`, `/api/peers`, `/api/config`)
- Internal `stats.rs` (exposed to plugins and the operator console)

`/api/metrics` (P3) returns the same payload as `/api/stats` plus a `metrics_version` and `generated_uptime_seconds` field for easier scraping / dashboards.

## Example Deployment

Start from the `supernode.toml` example in this guide and place it under the data directory before launching the service.

Typical systemd unit (example):

```ini
[Unit]
Description=DoubleSlash Supernode
After=network.target

[Service]
Environment=DOUBLESLASH_HOME=/var/lib/doubleslash
ExecStart=/usr/local/bin/doubleslash-supernode
User=doubleslash
Restart=on-failure
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
```

## Troubleshooting

- **Clients can't connect**: Check firewall for the QUIC port, certificate CN matching, and that the peer is allowed.
- **Room audio not working**: Verify `room.audio.sfu` is enabled in the manifest and the client negotiated the capability.
- **High memory**: Look at datagram receive buffers and concurrent streams in the transport config.

For more details, see the source:
- `src/main.rs` (startup & wiring)
- `src/manifest.rs` (typed config)
- `src/relay.rs`, `src/sfu.rs`, `src/web_app_module.rs`

## Contributing

Improvements to the supernode (especially better observability, hot-reload for plugins, or more access control modes) are welcome. Please keep the "no first-party backend for identity" principle.

---

*Maintained as part of the DoubleSlash project. Last updated for multi-platform release binaries (linux-x86_64, linux-aarch64, win64).*
