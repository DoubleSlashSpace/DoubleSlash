# DoubleSlash Terms of Use

**Effective date:** 2026-09-10

**Last updated:** 2026-11-15 (optional DDNS service terms added)

These terms describe acceptable use of DoubleSlash software and its
communication, sharing and hosted-content features, and the separate optional
paid DoubleSlash DDNS (DDDNS) service. The app is invite-only peer-to-peer software:
it requires no service account, and we do not centrally store your conversations.
Creating a DDNS account means accepting these terms for that service and
acknowledging the [Privacy Policy](PRIVACY.md).

The [Privacy Policy](PRIVACY.md) explains local storage, encryption boundaries,
device capture, and optional external services. Features and available controls
may vary by platform and version.

## 1. Your responsibilities

You create and control your identity on your device. Choose carefully whom
you invite and which peers or independently operated nodes you connect to.
You are responsible for your own conduct, the invitations you share, and the
content you send or make available.

Protect your identity files, unlock credentials, and backup passwords. Anyone
who obtains usable identity recovery material may be able to act as you.
There is no central password-reset or identity-recovery service.

## 2. Prohibited use

You may not use DoubleSlash to create, share, or distribute content that:

- Violates applicable law
- Depicts, promotes, or facilitates child sexual abuse or exploitation,
  or otherwise sexualizes minors
- Violates another person's privacy or consent, including non-consensual
  intimate imagery
- Contains malware or is intended to compromise devices, systems, or data

You may not use DoubleSlash to harass or threaten others. These rules apply
across all features and forms of content; a private or invite-only interaction
does not exempt anyone from them.

## 3. Local controls and moderation limits

Participant chat, file contents, voice, and video are encrypted between peers.
The project does not hold a central copy or the keys needed to inspect those
conversations. Maintainers cannot remotely erase content from another
person's device or impose a network-wide ban through a central account service.

Available controls may let you block direct contact, leave shared spaces,
disconnect from nodes, or delete locally stored content. These actions affect
your own participation or local data; they do not impose a network-wide ban or
delete copies held by others. Hiding an item from view does not end your
participation or remove remote content.

Reports can bring problems to the maintainers' attention, but do not grant
them access to sealed conversations or guarantee removal of remote content.

## 4. Reporting abuse

Where an in-app reporting option is available, it lets you prepare a report
and choose where to send it. It does not automatically attach private
conversations or files. You must complete the send; preparing a report alone
does not notify the maintainers.

You can also contact the project through [GitHub issues](https://github.com/DoubleSlashSpace/DoubleSlash/issues).
Issues are public: include only information you intend to publish. Do not
post identity keys, passwords, private conversations, intimate images, or
child sexual abuse material. A short description and relevant identifiers
are preferable to copying sensitive content.

Report suspected child sexual exploitation to the appropriate authorities
or child-protection reporting service in your country. In the United States,
use [NCMEC's CyberTipline](https://report.cybertip.org/). An in-app or GitHub
report does not submit a report to those services. If someone is in immediate
danger, contact local emergency services.

## 5. Shared content and third parties

Content you share is delivered to other participants, not stored by a central
DoubleSlash content-hosting service. Recipients can keep or copy what they
receive. Deleting your copy or withdrawing an offer cannot recall content
already delivered.

Independently operated nodes, including supernodes, may host shared spaces or
relay participant traffic. Operators can observe routing and membership
metadata even when the content is encrypted. Choosing a relay does not make
its operator a trusted recipient of your private conversation.

Third-party content and applications, including portals and games, may run
inside the client or connect to external services. Do not assume they have
the same encryption protections as chat and calls. Review what they ask you
to share, and use only providers you trust for that interaction.

## 6. Age

DoubleSlash is not directed at children under 13. Do not invite a child under
13 to use it.

## 7. Software license and warranty

DoubleSlash is distributed under the [MIT License](LICENSE). That license
governs permission to use, copy, modify, and distribute the software; these
usage guidelines do not amend it.

The software is provided “as is,” without warranty, subject to the disclaimer
and limitation of liability in that license.

## 8. Optional DoubleSlash DDNS service

DoubleSlash DDNS, operated under the DoubleSlash name at
`users.doubleslash.space`, publishes address records that help clients reconnect
to your supernode after an IP change. It is optional and helps support development.
Buying it is not a condition of using the software. You may instead use ordinary
DNS for a domain you control or host a compatible JSON record on your own website.
The [Privacy Policy](PRIVACY.md#optional-doubleslash-ddns-service-and-website)
describes service data and providers. Service support and privacy contact:
[admin@doubleslash.space](mailto:admin@doubleslash.space).

### Price, renewal and cancellation

The current plan is **US $10 per account per year for up to 10 names**, rather
than $10 per name. Stripe Checkout shows the amount and any applicable taxes
before you authorize payment. Creating an account alone does not charge you.
You must be legally able to enter into the subscription, or act with the
authorization of someone who can.

**The subscription renews automatically each year until cancelled.** To stop
renewal, sign in at `users.doubleslash.space`, open **Payments, card and invoices**,
and cancel in Stripe's billing portal before the next renewal. The current portal
cancels at the end of the paid period. Contact the service email if you cannot
access the portal or need help stopping renewal. Releasing a name, revoking a
token, deleting the app or no longer using the software does **not** cancel the
subscription. You can continue using the paid plan through its paid period,
subject to these terms.

After a failed renewal the service currently allows a 14-day grace period, after
which affected names stop serving addresses unless payment is restored. Released
names are normally held out of circulation for 30 days before becoming available
again. Payment activation is confirmed by Stripe; returning from Checkout alone
does not establish that a payment succeeded.

Cancellation does not automatically refund previous charges. For a billing error,
duplicate charge, unavailable purchased service or another refund request, contact
[admin@doubleslash.space](mailto:admin@doubleslash.space) with a Stripe receipt or
invoice reference. Do not send card details or account keys. Nothing in these
terms excludes any refund, cancellation or other consumer right required by
applicable law. Test-mode purchases are simulations and do not transfer into
live subscriptions.

### Account keys, names and permitted use

Save the account key shown at signup and keep update tokens private. There is no
email password reset. Support may be able to reissue a key after verifying account
ownership or a Stripe receipt; recovery is not guaranteed without adequate proof.
You are responsible for authorized access to your account and the accuracy of
the addresses and port you publish. Changes must be submitted through the site
or an authorized update request; the service does not itself detect your server's
IP changes. DDNS does not host, secure or operate your supernode.

Names are service identifiers, not domain registrations or permanent ownership
rights. Do not impersonate others, infringe their rights, publish endpoints you
are not authorized to manage, misuse tokens, evade rate limits, or use the service
for fraud, malware, harassment or other prohibited conduct described above.
We may reserve names and suspend or release records for abuse, security, legal
requirements or nonpayment. These actions affect this optional directory service;
they do not give us control over private app identities or conversations.
Report service abuse privately to the contact above.

### Public records, availability and service changes

**Published names, IP addresses and ports are publicly retrievable through lookup
records.** A browser redirect to the main site does not make the record private.
The service is an address-discovery aid, not an anonymity service or proof of a
supernode's identity. You still control your node and decide whom to trust.

Maintenance, network failures, stale caches or suspension can interrupt lookups.
We do not promise uninterrupted availability or that every existing invite will
reconnect after an address change. Avoid relying on DDNS as your only recovery
path. To the extent allowed by law, the hosted service is provided without an
availability guarantee; this does not remove mandatory consumer rights or the
software permissions granted by the MIT License.

Material changes to service terms, prices or features will be described on the
service site before they apply to a future purchase or renewal. Review the current
terms and the amount shown at Checkout. You can cancel future renewal if you do
not wish to continue. For service closure, access, correction or deletion requests,
contact the service email; we will verify control and address continuing billing
and any required record retention. No self-service account deletion is currently
provided.

## Contact

For DDNS account, billing, privacy and private abuse requests, use
[admin@doubleslash.space](mailto:admin@doubleslash.space). For project questions or
non-sensitive reports, use
[GitHub issues](https://github.com/DoubleSlashSpace/DoubleSlash/issues).
For details about personal data and optional network contacts, read the
[Privacy Policy](PRIVACY.md).
