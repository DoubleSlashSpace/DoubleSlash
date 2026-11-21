# DoubleSlash threat model

Scope: client identity and storage, invites, direct/relay signaling, rooms,
media, portals, plugins and software delivery. This describes current boundaries,
not a completed security audit. Implementation invariants live in the
[agent contract](../agents.md); private reports follow [SECURITY.md](../SECURITY.md).

## Assets

- Identity seeds, local endpoint keys and accepted device registries.
- Peer trust, invite credentials, private-room admission and sender keys.
- Participant chat, file contents, voice, camera/screen video and shared audio.
- Backups, attachments, local histories and managed-server SSH credentials.
- Software packages, native modules and operator configuration.

## Trust boundaries

Clients own identities and trust; there is no mandatory identity, discovery or
presence service. A chosen supernode is a transport/hosting peer, not a content
recipient or identity authority. It can observe peer/device IDs, room/session IDs,
membership, message/control types, signatures, traffic timing and byte counts.
Room names and file-offer/completion metadata can be visible.

Portal assets are operator-provided content. Game payloads are opaque to the
forwarder but the SDK does not seal them against it. An invited peer can retain
or disclose anything it receives. A compromised endpoint can use its unlocked
keys; network encryption does not protect against that endpoint.

## Threat surfaces and controls

### Invites and admission

Personal invites are signed, expiring and carry ephemeral X25519 material.
The inviter also requires a live, single-use issuance record or an already
trusted peer; a valid joiner signature alone is not an invitation. Restart
invalidates unredeemed personal links. Signatures, transcript binding and key
confirmation protect handshake integrity. A sibling-device refusal waits for
the minting device's accept before becoming a visible failure.

Room-member trust requests contain a personal invite sealed to the recipient.
They require authoritative room membership, no existing sender record, a live
sender-owned invite, sanitized names and rate limits. Receiving only prompts;
acceptance establishes trust. Private-room admission separately requires Space
proof/grant, local token/allowed state or creator admission. A room ID, roster
or peer trust record alone is not a membership grant.

Residual risks include leaked bearer invitations, social engineering, and a
malicious authorized member redistributing content or invitations within policy.

### Signaling and endpoint discovery

Before using a supernode WebSocket, the client verifies a fresh signed HELLO
challenge against the expected node identity. Invites, DNS and DDDNS supply
address hints only. Over plain `ws://` an observer forwarding to the genuine
node can still inspect signaling; the proof is authentication, not encryption.

Signed envelopes require valid sender signatures and timestamp freshness.
Per-sender signature replay deduplication exempts high-rate SFU audio and
idempotent file chunk/completion classes, but not their signatures or freshness.
Foreign chat, call and file classes require mutual trust. Device fields are
signature-bound; sealed own-device traffic requires our identity and another
non-empty source device. Signaling queues have an identity-wide 8 MiB bound.

Residual risks include traffic analysis, denial of service and wall-clock skew.
Replay deduplication is not a total ordering protocol.

### Direct and relay transport

QUIC uses rustls/quinn and checks identity/device claims from authenticated
certificates. Direct sessions have transport encryption. Relayed direct
datagrams use `derive_pairwise_relay_key` from static identity keys; that
construction does not provide forward secrecy. Ephemeral invite-handshake
keys have a different scope and do not turn every relay payload into a
forward-secret session.

Relay attachment uses signed tickets and authenticated identities. Scoped
membership, indices, feature authorization, quotas and bounded queues constrain
forwarding. The ordered room-file chunk/completion exception observes quota
refusal without discarding those frames; pacing, frame limits, queues and
integrity checks remain required. Nodes can still drop, delay or correlate
traffic. A trusted relay does not authorize a foreign application sender.

### Room keys and cluster replication

Room chat, file contents and media require real distributed sender keys. Inbound
cleartext or failed opens are rejected. Group-key installation checks normalized
membership, elected keyer/device and bounded epoch advancement. Media AAD
separates voice, video and shared audio. Key ACK/reseal retries and member-driven
sealed key requests recover missed epochs; own-device handoff additionally binds
a fresh challenge and roster. The low-level epoch-0 fallback is not permission
to send private content.

Cluster links authenticate roster members and sign control messages. Replicated
participant frames remain opaque, deduplicated and non-recursive. Private-room
allowed sets are not replicated. Nodes do not persist participant payloads or
room definitions. Routing correctness and malicious-member denial of service
remain risks; inspect both sender/keyer and receiver logs when diagnosing gaps.

### Portal pages and native modules

Portals load through native scheme handlers over authenticated QUIC, without a
public HTTP/WebTransport listener. Game sessions are scoped by the native
channel runtime. Guest IDs in demo state are not verified peer identities.
Portal HTML is untrusted operator content, and demo state in open pages is
neither durable nor an anti-cheat boundary.

Navigation and bridge exposure are consumer-specific: Android restricts its
portal WebView to native portal schemes, and desktop portal/game adapters expose
only their channel API. A top-level navigation allowlist alone does not prove
that every subresource is blocked. Keep external browsing and local previews
separate from privileged bridges.

Native feature modules require signature/trusted-signer checks and client
consent for bespoke features. They execute in-process. There is no native plugin
sandbox or implemented WASM isolation. A trusted malicious binary can use the
host process's authority.

### Local storage, backups and device authority

The v2 identity uses Argon2id and AES-256-GCM. Peer/room files and history fields
are encrypted under identity-derived storage keys. OS keyrings/Android Keystore
may cache a protected derived unlock key, never the passphrase. The derived key
can unlock a copied matching identity file and must remain secret. Settings,
attachments and diagnostic logs are not covered by message-field encryption.

Backup export is explicit. Authenticated streaming frames, bounded extraction,
path checks and staged verification precede a new-profile restore. Local device
keys, keyring entries and native modules are not copied. Anyone holding an archive
and its password can recover full identity authority; importing that identity
is not revocable delegated pairing.

Device registries persist predecessor, revocation, rollback and fork checks.
They are not integrated into delegated live authentication. Restoring an entire
old profile defeats purely local monotonic-version checks. `open_with_key`
separates storage access from signing at an API boundary but does not distribute
independent storage keys. Current device routes are root-authorized.

### Capture and optional integrations

Capture starts through call/share/preview controls. Active capture can continue
when the app is backgrounded, including Android foreground-service calls.
Whole-screen sharing includes overlays and notifications. Windows application
audio capture falls back to whole-machine audio before build 20348. Media is not
recorded by the client. Recipients can still record their output.

Ollama receives the text/images/optional transcribed audio routed to its configured
endpoint. Enabled tools can drive calls, chat and explicitly permitted file
sharing; model decisions are not a separate authorization boundary. SSH setup
controls a rented host with profile-held credentials and pinned host keys; its
hardening is a baseline, not ongoing server administration. DDDNS publication
uses a transient update token and makes address records public. See
[PRIVACY.md](../PRIVACY.md) for disclosures and defaults.

### Software delivery

The official Windows Store package is signed and updated by Microsoft.
Unpackaged GitHub Windows builds remain unsigned pending SignPath acceptance.
Workflow support for optional signing/attestations does not prove a particular
artifact was signed or unmodified.

Installer stable-release manifests, when supplied, require the project Ed25519
signature and matching archive hash. A stable install can proceed without a
manifest. Nightly requires a matching manifest hash but accepts an unsigned
manifest. Sidecars are checked when supplied. The manager instead checks release
SHA-256 sidecars and distribution notices; project signed-manifest verification
is not implemented there. See [Updates](../README.md#updates) and the
[manager guide](supernode-manager.md#release-download-and-updates).

Residual supply-chain risk includes missing manifest enforcement, compromised
build credentials or dependencies, hostile native modules and unverified
platform builds. Peer build attestations are signed claims, not remote execution
proof. The project uses pinned CI actions and supply-chain/license checks; those
controls do not replace artifact and source review.

## Validation and remaining work

Negative-path tests must preserve signature/freshness/replay, trust and
membership gates, failed key opens, device handoff/registry failures, quotas,
path containment, update verification and restart isolation. Existing tests do
not establish every manual scenario. Use [acceptance](ACCEPTANCE.md) for live
results and the [backlog](../backlog.md) for pending device, media, platform,
release and cryptography work. Post-quantum changes are [deferred design](POST_QUANTUM.md).
