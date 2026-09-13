# Devices and backups

The first delivery provides portable encrypted backups, verified restore, and
file-based moves between desktop and Android. It does **not** enable simultaneous
connections from devices sharing an identity. Device linking and ongoing sync
require the protocol changes listed below.

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
history. Room sidebar tombstones and owner-held Space trees are included. Files
deleted from disk cannot be reconstructed; missing or excluded attachment paths
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

The backup format deliberately does not masquerade as a device credential.
Current stores derive encryption keys from the identity signing seed. Before
linking a phone without granting it full identity authority, introduce portable
storage key material independent of that seed and migrate existing stores.

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

Before release, exercise desktop → real Android → desktop restore with a large
attachment, a missing attachment, a keyfile-protected source, a failed/cancelled
document-picker copy and a restart after profile selection. Check actual history
and files, not only the success banner. Simultaneous device use is outside this
delivery's acceptance scope.
