# DoubleSlash Security Policy

**Effective date:** 2026-10-29

This policy covers the DoubleSlash desktop client, Android client, supernode,
installer, protocol libraries, web SDK, and the public site in this repository.

## Reporting a vulnerability

Report a suspected vulnerability in private. Do not open a public issue, pull
request, or discussion for an unfixed vulnerability, and do not put exploit
details in a public commit.

Use GitHub private vulnerability reporting:

[Report a vulnerability](https://github.com/DoubleSlashSpace/DoubleSlash/security/advisories/new)

If that form is unavailable, open a public issue that says only that you have a
security report and how to reach you. Leave out the vulnerability, the proof of
concept, and the affected paths. A maintainer will answer with a private channel.

Include:

- The component, version, commit, or nightly date.
- What an attacker can do, and what they already need, such as a trusted peer, room membership, or a supernode the victim chose.
- Steps to reproduce, or a minimal proof of concept run against clients you control.
- Whether the issue is already public.

There is no bug bounty.

## What happens next

We will acknowledge a report after we have read it. This project does not
promise a response time. When a report is confirmed, the fix goes on the
`develop` branch, and we will agree a public disclosure with you. You are
credited unless you ask not to be.

Give us a chance to publish a fix before you publish details. Ninety days from
acknowledgment is the usual window. We will move faster when an issue is being
exploited, and we can agree a longer window when a fix needs it.

## Supported versions

DoubleSlash is pre-release. Security fixes land on `develop` and ship in a
later [GitHub Release](https://github.com/DoubleSlashSpace/DoubleSlash/releases).
Older releases are not maintained as their own line. Nightly builds are
pre-release and are replaced by the next nightly.

| Line | Security fixes |
|---|---|
| `develop` | Yes |
| Newest GitHub Release | Yes, by a newer release |
| Older releases | No |
| `nightly` | Only by a later nightly |

## In scope

- The desktop client, Android client, supernode, installer, `doubleslash-features`, and the web SDK.
- The invite handshake, signaling authentication, replay and freshness checks, room keys, and file-transfer integrity.
- A supernode, an untrusted peer, or a portal page reading or changing content the design treats as sealed.
- The portal bridge being attached to a page outside `d://`.
- Update verification accepting a tampered stable archive when a manifest is supplied.

## Out of scope

These are known properties of the design. They are described in the
[threat model](docs/THREAT_MODEL.md) and in [Security & Identity](README.md#security--identity).

- A supernode seeing routing metadata: peer and device ids, room and session ids, membership, room names, file-offer metadata, message types, signatures, timing, and byte counts.
- Relayed direct datagrams using a pairwise key derived from static identity keys. That path does not provide forward secrecy.
- `game.relay.v1` forwarding application datagrams without sealing them. The application has to provide its own confidentiality.
- A person you invited, or a supernode you chose, acting inside the trust you already gave them.
- Social engineering of an invite, or compromise of the device or operating system that holds the identity.
- Unfinished work listed in [backlog.md](backlog.md).
- Flooding a supernode you do not operate. Describe a protocol bug with a reproduction on machines you control.
- A bug in a third-party dependency that a DoubleSlash build cannot reach. Report that upstream. Report it here as well when DoubleSlash can reach it.

## Research

Test only clients, identities, and supernodes you own or have permission to
use. A local reproduction is enough. Do not read someone else's messages,
files, or identity to demonstrate an issue, and do not disrupt someone else's
service.

We will not pursue legal action for research that follows this policy. This is
a project policy. It is not permission to test other people's clients or
supernodes.

## Related documents

- [Threat model](docs/THREAT_MODEL.md)
- [Security & Identity](README.md#security--identity)
- [Privacy policy](PRIVACY.md)
