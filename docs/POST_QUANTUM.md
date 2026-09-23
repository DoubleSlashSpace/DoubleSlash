# Post-quantum crypto (ML-KEM / ML-DSA)

Assessed 2026-07-11. Nothing here is built; this is the design reference for
when the [backlog](../backlog.md#post-quantum-crypto) item is picked up.

The codebase is fully classical (Ed25519 + X25519 + AES-256-GCM/HKDF-SHA256).
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

## Findings

In this order if built:

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

## Build order

(1) hybrid TLS provider, (2) hybrid invite handshake, (3) KEM-DEM pairwise /
group-key wrap, (4) dual-sign low-rate + manifests, (5) identity ML-DSA
binding, (6) browser parity, (7) deprecate pure classical by policy only after
ecosystem age.

Space Layer 1 helps: admission/directory trust costs **one signature per Space
per epoch**.

## Non-goals

Pure ML-KEM without X25519 hybrid on first ship; ML-DSA on every SFU audio
frame; reviving TreeKEM for PQ rooms; QUIC-stack PQ as a hard dependency of
app-layer PQ; SLH-DSA / FN-DSA unless a later trade-off demands it.
