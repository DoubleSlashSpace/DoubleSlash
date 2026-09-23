# Devices and backups

The first delivery provides portable encrypted backups, verified restore, and
file-based moves between desktop and Android. Device-aware routing, on by
default, lets a desktop and a phone that share an identity (imported through a
backup) stay connected at the same time; see
[Simultaneous-identity preview testing](#simultaneous-identity-preview-testing)
for what has passed. Device linking and ongoing sync require the protocol
changes listed below.

## Create a backup

1. Unlock the identity you want to back up.
2. Desktop: **Settings → Identity → Devices & Backups**. Android:
   **Settings → Devices & Backups**.
3. Choose **Create encrypted backup**, select a new `.dbackup` filename, and
   enter and confirm a backup password of at least 12 characters.
4. Leave attachment inclusion enabled for a full backup. Wait for verification
   and check the missing-attachment count before relying on the file.

The backup captures live peer and room stores, a SQLite backup snapshot including
committed WAL pages, preferences, and available attachment files referenced by
history. Room sidebar tombstones and owner-held Space trees are included.
Accepted device registries and revocations are also included when a profile has
`device-trust.db`; it is captured using SQLite's online backup API. Damaged trust
data prevents backup publication or restore. Archives containing this new optional
entry require a client version that supports it; older archives remain readable.
Files deleted from disk cannot be reconstructed; missing or excluded attachment paths
are cleared in the restored history. Attachment display names are preserved.
Logs, runtime relay tickets, OS keyring blobs, native plugin binaries, and files
unrelated to chat history are not included.

Keep the backup password separately. The archive includes recovery material for
the identity, so the old unlock passphrase and keyfile are not required. There
is no password-reset service. Backups contain one selected identity/profile;
repeat export for other profiles you want to retain.

## Restore or load an identity

From the unlock screen choose **Restore backup or load another identity**.
An already-running session must be locked first. On desktop, **Lock and quit**
forgets the cached unlock key; reopen the app to reach the unlock screen.

Open the backup, enter its password, and wait for the verified preview. Check the
identity and message/attachment counts. Choose and confirm a new local unlock
passphrase, then restore into a new profile. Desktop requires closing and
reopening the app after selecting a profile; Android can unlock it immediately.
The local passphrase is independent of the archive password.

Existing profiles are retained. The same wizard lists the original profile and
restored profiles for selection while locked. Restored profiles live under
`profiles/<uuid>/`; the collection root's `active-profile` file contains a profile
UUID or `original`. The existing environment-variable directory resolution still
selects the collection root. Desktop settings writes stay pinned to their loaded
profile until the next launch, so closing the old UI cannot overwrite restored
preferences.

Camera sharing, source selections, machine-specific device/path settings and
automatic AI responses are reset on restore. Native modules must be installed
and authorized on the destination. Android permission grants, legal acceptance,
notification disclosures, and Keystore auto-unlock are established locally.

## Move between desktop and phone

Create a backup, copy the encrypted file using your preferred file-transfer
method, and restore it on the other device. **Quit the old device before
connecting the destination.** Keep the old profile until you have verified your
contacts, rooms, messages, and attachments on the destination.

This works with release Android builds through the system document picker; it
does not require USB debugging or `run-as`. `scripts/push_android_profile.ps1`
remains a developer helper for debug builds, not the recommended user flow.

## Archive v1 and failure handling

- Public header: 8-byte `DSSHBA01` magic/version and 32-byte random salt. V1 fixes
  Argon2id parameters to 64 MiB, three iterations, four lanes; archive input cannot
  request an arbitrarily expensive KDF.
- Frames: 4-byte big-endian length, 12-byte random nonce, AES-256-GCM ciphertext
  and authentication tag. Associated data binds the entire header and monotonically
  increasing 64-bit frame index. Plaintext frames are at most 1 MiB.
- Encrypted manifest: identity public ID, timestamp, counts, entry names and
  lengths. Followed by the 32-byte identity seed, each entry's chunks, and an
  authenticated completion marker. Trailing bytes, truncation, reordered frames,
  invalid lengths and failed authentication are rejected.
- Extraction accepts only the defined store/settings names and numeric
  `attachments/<index>` entries. Duplicate paths, traversal and absolute paths
  are rejected. Counts and sizes are bounded (100,000 entries, 1 TiB aggregate;
  the manifest must also fit one frame).
- Export writes a temporary encrypted file, reopens and verifies every frame,
  then publishes without overwriting an existing destination. Android copies
  that verified file to the chosen document URI and reports copy errors.
- Import verifies the full stream, identity consistency, peer/room decoding,
  database integrity, encrypted message fields and attachment references before
  offering a preview. Commit writes a new encrypted identity file, remaps attachment
  paths and publishes a fresh profile. An import never merges or replaces an
  existing profile. Normal cancellation/failure drops the staging directory;
  abrupt process termination can leave staging data in private app storage.

## Remaining device-linking work

The current development groundwork is in `src/device.rs`: independent Ed25519
device keys, an identity-signed device registry, permanent revocation tombstones,
monotonic version checks, same-version fork detection, and possession proofs bound
to a challenge and handshake transcript. Session startup can load a distinct
persistent endpoint key behind the disabled `DEVICE_ROUTING_READY` release gate.
Delegated registry authentication is not wired into live sessions, and nothing
advertises multi-device support yet. `DeviceKey::load_or_create` offers encrypted, atomic owner-profile
persistence; `device-key.dat` is deliberately excluded from portable backups so
restoring creates a distinct endpoint key.

`DeviceTrustStore` in `src/device/store.rs` now persists accepted registries in an
encrypted `device-trust.db`. Immediate SQLite transactions check every update
against the latest committed predecessor, including updates from another process.
Same-version forks, older versions and attempts to undo revocations are rejected. Failed
writes do not publish authorization. Proof verification reads current committed
state, and backup restore validates all encrypted registry rows and signatures.
This store has not yet been wired into network authentication. Callers must pin
the contact identity independently; a valid registry does not establish contact
trust. Whole-profile rollback remains outside local version protection: restoring
an older backup requires catching up with trusted devices before relying on it
for current authorization.

Peer, room and chat stores now also accept their individual storage subkeys through
`open_with_key`, so opening them no longer requires possession of an `Identity`.
Existing entry points and file formats remain compatible. This separates access
to local data from signing authority at the API boundary; it does not yet implement
a companion credential vault or distribute keys to another device.

The backup format deliberately does not masquerade as a device credential.
Current stores derive encryption keys from the identity signing seed. Before
linking a phone without granting it full identity authority, introduce portable
storage key material independent of that seed and migrate existing stores.

The shared transport groundwork now includes signed source/target device fields
and identity-scoped endpoint routing tables. Signaling signatures bind both fields;
legacy messages retain their existing canonical encoding. Routing tests cover
independent transport fallback for each endpoint, exact-device targeting,
padding/hex identity aliases, route limits, and stale reconnect cleanup.
`device-routing` is on by default, as the `doubleslash-features` default feature
(since 2026-09-15). Builds enable root-authorized endpoints and advertise
`core.devices.v1`. A build without it and a build with it do not interoperate on
one identity: the two devices keep dropping each other's connections.

The signaling connection lifecycle now supports separate sockets per device:
identity-wide delivery reaches both, a device target selects only that endpoint,
and replacing or closing one device preserves its sibling and identity presence.
Two real localhost WebSocket tests exercise these transitions with one identity.
WebSocket connections negotiate `doubleslash.devices.v1` before registering, so an old node
cannot accidentally replace another device's session. Registration requires the identity's
signature, so this is not yet delegated companion authentication.

The 2026-09-14 development changes extend this to QUIC relay connections and
direct peer sessions. Device certificates bind a separate endpoint ID to the
root identity's TLS signature. Relay endpoints have separate datagram indices;
direct sessions retain multiple routes under one contact. Replacing one route
does not disconnect a sibling, and stale disconnects cannot clear its replacement.
Direct signaling supports endpoint targeting and fan-out across connected routes.
An arriving sibling does not change an existing selected media path.

Room membership now tracks voice and chat subscriptions per endpoint. Local and
cluster-replicated room chat excludes only its originating endpoint, reaches other
subscribed devices under the same identity, and charges outgoing quotas per copy.
Desktop history marks messages received from another own device as sent by the
user; they do not trigger automatic replies.

Room-creation and restoration acknowledgements return only to the requesting
endpoint, including denials. Broadcasting these replies to sibling devices could
make an unsolicited restoration acknowledgement trigger a voice join on a device
that never requested one. Shared room rosters still reach all subscribed devices.

`connection_manager/manager/device_session.rs` coordinates room keys between own
devices. Before an elected endpoint creates a key, it asks its currently rostered
siblings for an existing key using an encrypted, signed, device-targeted exchange
under the room chat quota. Requests bind a fresh challenge and the current device
roster. Malformed, stale, or unencrypted replies cannot authorize key creation.
Equal-epoch key conflicts fail closed. Leaving a host clears its roster and pending
handoff; late replies cannot reinstall a key after leaving the last host.

`manager/device_calls.rs` selects one answering endpoint, confirms that selection
with bounded retries, dismisses sibling ringing, and binds direct media and
reconnect fallback to the selected endpoint. Reliable chat/control delivery also
uses encrypted relay copies to reach siblings without a direct route.

An endpoint that loses the selection reports `CallAnsweredElsewhere` rather than
`CallEnded`: it stops ringing (the desktop closes its incoming-call dialog
without sending a reject) and does not count a missed call. Android receives it
as the same `call_ended` event.

Installed desktop/phone acceptance has partly passed; see
[Simultaneous-identity preview testing](#simultaneous-identity-preview-testing).
Remaining work includes device endpoint discovery, device-scoped
file/room-media/game state, and coordinated cluster device rosters.
Continuous history sync and delegated device revocation remain separate unfinished
parts of the linking workflow. Do not enable the release gate on the strength of
room-chat tests alone.

Same-identity hardware acceptance uses the installed desktop app with the
user's normal profile and the connected phone. Do not substitute the separate
`.clientA` profile used for the earlier distinct-identity baseline.

Registered WebSocket and QUIC signaling writers now share an 8 MiB queued-byte
ceiling per identity. Reconnecting cannot reset queued reservations while old
writers remain alive. Draining or dropping a frame releases its reservation;
overflow closes the affected connection. WebSocket cleanup also runs when its
writer exits while the client is idle, and authenticated sockets cannot change
their signing identity mid-connection.

The subsequent protocol delivery needs:

1. Identity-authorized device keys, a signed device registry, revocation and
   monotonic registry versions. Preserve the contact-facing identity.
2. Capability-negotiated device addressing in direct sessions, relay maps,
   signaling, endpoint mailboxes and room key delivery. Old clients must reject
   unsupported device linking visibly; copying a seed cannot be the fallback.
3. Short-lived QR pairing, approval on both devices and a dedicated authenticated
   capability with explicit device authorization and quotas. Supernodes forward
   opaque bytes only.
4. Idempotent history sync with stable message IDs, deletion/block tombstones,
   resumable attachment transfer and deterministic offline conflict resolution.
5. Multi-device call fan-out with one answering endpoint, per-device media keys,
   explicit device removal, and tests for offline catch-up and revoked devices.

Without a history-holding backend, catch-up requires another device holding the
missing data to become reachable. Offline queueing, if later added to volunteer
supernodes, must be opt-in, bounded and end-to-end encrypted.

## Validation

Automated backup tests cover fresh-profile recovery without old credentials,
committed SQLite data, attachments spanning frames, hidden rooms, block state,
wrong passwords, tampering, truncation, trailing bytes, frame reordering, path
validation, source-file changes, non-overwrite publication, preview tokens,
profile isolation and staging cleanup.

Device groundwork tests cover two keys under one identity, wrong-identity and
tampered registries, revocation, rollback, conflicting registries, transcript and
challenge binding, malformed keys/names, bounded documents, concurrent key-file
creation, encrypted persistence, and existing-store access after dropping the
identity seed. Backup tests also verify that restore does not clone a device key.

On 2026-09-13, the connected Windows desktop and Android phone completed direct
chat and private-room chat round trips using their existing, distinct identities.
This establishes the transport baseline before device routing changes; it does
not validate same-identity sessions, backup restore, or voice. The installed apps
used for that baseline predate the new routing groundwork. Updated shared-feature
and supernode tests cover queue isolation, concurrent writers, reconnects, and
idle WebSocket teardown; no updated application or supernode has been deployed
for same-identity acceptance yet.

The renamed-tree regression run passed 672 client tests (7 existing ignored),
165 shared-feature tests, 280 supernode tests and 84 installer tests. Test
inventories were re-derived with `cargo test -- --list`; the shared feature
crate also has 3 existing ignored doctests. Strict feature/supernode lint and
`scripts/ci_local.ps1 -SkipTests -SkipAudit -SkipOpusFetch` passed, including
client and macOS cross-lint, formatting and the manifest-signing self-test.
Release-mode whole-workspace tests and an advisory audit were not run in this
pass. Rename-related single-item loops in client/installer lookup and cleanup
code were simplified to satisfy the lint gate without suppressions.

The subsequent durable-trust change passed 681 client tests (7 existing ignored;
688 listed), strict client library/test lint and Android arm64 native lint.
New tests cover persisted revocations, competing/stale writers, failed writes,
wrong keys, corrupted or oversized rows, identity binding, missing/empty stores,
WAL-backed registry recovery and rejection of malformed trust data before backup
publication. These checks do not substitute for live device authentication or
same-identity desktop/phone acceptance.

The 2026-09-14 preview checks passed 695 client tests (7 existing ignored),
165 shared-feature tests and 289 supernode tests, with strict library/test lint.
Windows Qt/WebEngine and Android arm64 lint also passed. New coverage includes real
QUIC device identity authentication and forgery rejection, independent direct
routes, relay reconnects, per-device room membership and chat delivery, encrypted
own-device room-key handoff, exact-once own-message reception, and rejection of
late or malformed handoffs. These are automated development checks, not installed
desktop/phone acceptance.

### Simultaneous-identity preview testing

Every client and node must include `device-routing`. It is the
`doubleslash-features` default, so plain builds do; the older
`DOUBLESLASH_DEVICE_ROUTING=1`, `-Pdoubleslash.deviceRouting=true` and
`--features device-routing` switches are now redundant. An installed app that
predates the default, such as an earlier nightly, still lacks it. Both devices
currently need the full identity imported through the encrypted backup workflow.

Keep the normal desktop profile and phone unlocked together. Verify both remain
connected, room chat appears on both with own messages marked as sent, an incoming
call can be answered on either device and stops ringing on the other, and reconnecting
one device leaves the other connected. Repeat with a third contact and with one
device using a relay. Continuous history sync and all room-media/file/game flows
are not covered by this preview's automated acceptance.

**Result, 2026-09-22.** The installed desktop (normal packaged profile) and the
phone, both on one identity, passed the connected, room-chat, reconnect,
third-contact, and relay checks; the user ran them. The desktop's log for that
day shows both devices online throughout, own-device room-key rounds over two
devices, and a room file pulled from the phone with a third contact in the
room. **Still open:** answering an incoming call on either device. Reviewing
that path found that the desktop's ringing dialog stayed open after the call
ended elsewhere, and then sent a reject when it timed out 30 s later. The
answer-elsewhere event above fixes that; repeat the call check on a build that
includes it.

`scripts/test_backup_wizard.ps1` runs the shipping desktop wizard with a mocked
backend under Qt Quick Test, including preview confirmation, password clearing,
retry after failure, busy-state controls, and compact-window sizing. The mock
does not substitute for the shared Rust archive tests or real document pickers.

Before release, exercise desktop → real Android → desktop restore with a large
attachment, a missing attachment, a keyfile-protected source, a failed/cancelled
document-picker copy and a restart after profile selection. Check actual history
and files, not only the success banner. A plain desktop → phone → desktop round
trip has been done (the current desktop profile was restored from the phone);
the listed edge cases have not. Simultaneous device use is outside this
delivery's acceptance scope.
