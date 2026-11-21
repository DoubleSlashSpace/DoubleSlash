# Supernode administration: remaining design

Desktop SSH administration is implemented. Its setup, import, update, lifecycle,
hostname and cluster workflows are documented in the
[manager guide](supernode-manager.md#desktop-administration). The shared service
is `rust/doubleslash-client/src/supernode_admin/` over the manager libraries;
it is not an extraction still waiting to happen. Current live-check criteria
belong in the [backlog](../backlog.md#host-a-supernode-from-the-client).

This document keeps the remaining client/provider design. It is not an operator
runbook or evidence that Android administration or cloud provisioning is available.

## Remaining client work

- Expose administration on Android through the existing shared Rust service and
  JNI command/event channel, with request IDs, progress, resumable errors and no
  blocking of UI or connection-manager loops. Android deploys release packages;
  desktop retains local-source builds.
- Import an existing manager `inventory.toml`, with credentials configured
  separately on each device. This differs from implemented read-only discovery
  of an already running Linux/systemd deployment.
- Add client-side feature/room-policy editing without losing custom settings or
  cluster rosters. Imported deployments remain restricted to the explicitly
  supported read-only and hostname operations until a separate management
  transition is designed.
- Complete the packaged wizard and Linux/cluster/update acceptance in the backlog.
  Library/fake-transport and mock-QML results do not prove those workflows.

## Shared service requirements

Keep the server independent of DoubleSlash identity authority. After deploying,
read its reusable invite and use the ordinary signed client handshake. A setup
success message must distinguish installed service health, successful hostname
publication and an authenticated live client connection.

Preserve pinned SSH host keys and reject changed ones. Passwords remain transient;
profile-held SSH keys and managed metadata stay encrypted and excluded from
portable backups. Verify fresh key-only access before optional SSH lockdown.
Keep update hash/notices checks, locking, previous-build rollback, persistent
identity/state and member-restricted cluster firewall rules.

The existing responsibility notice explains that setup/hardening is a baseline,
not a managed service: the operator retains server security, updates, backups,
costs, compliance and usage responsibility. Preserve that notice and per-profile
acceptance when adding another client. Do not imply that a successful install
accepts responsibility for ongoing administration.

## Provider signup and future provisioning

The manual **I have a server** SSH path remains available. Provider API
provisioning is separate future work; current setup does not collect cloud
account credentials or create billable instances through provider APIs.

The supplied optional Vultr signup link is
`https://www.vultr.com/?ref=6956347`. Its action must disclose that DoubleSlash
may earn a commission and open the system browser, returning to the SSH setup
step. Verify the link and attribution before release. No DigitalOcean referral
link has been supplied; do not imply one earns a commission. Opening a signup
link does not establish API provisioning or referral attribution for an API create.

### Adapter contract

A future provider adapter handles account authorization, resource discovery,
provisioning, cloud firewall rules and provider resource IDs. It returns an SSH
target, public address, architecture and provider metadata to the shared SSH
flow; node install/update/rollback and trust establishment remain provider-neutral.

- Fetch available regions, compatible Linux images and sizes with current prices.
  Show selected extras and ongoing charges immediately before explicit approval
  of a billable create.
- Persist the provider ID and progress before continuing into SSH. Poll async
  creation and SSH readiness. Retry/resume must not create duplicate servers.
- Keep a failed deployment's still-billable resource visible. Offer separately
  confirmed destruction; do not silently delete a host or regenerate node state.
- Use system-browser authorization with state/PKCE where the provider supports
  secure native public clients. Never embed an application secret. Store tokens
  in OS-protected storage, apart from inventory, logs, invites and backups.
- Request only needed permissions. Keep resource deletion and any broader cloud
  access explicit. A user-supplied token needs a separate scope/IP-access review.
- Configure provider and host firewalls: selected SSH access, signaling TCP and
  relay UDP; cluster UDP only from sibling addresses. Do not open a public
  HTTP/WebTransport port for the in-app portal.

### Provider research to revisit

Earlier research identified potential Vultr instance/cloud-init/firewall and
DigitalOcean Droplet/OAuth flows. It has not established a working integration,
current API support or acceptance on either client. Re-read current primary
provider documentation when implementing:

| Provider | Starting references | Decision to resolve |
|---|---|---|
| Vultr | [Instance provisioning](https://docs.vultr.com/products/compute/instances/cloud-compute/provisioning), [PKCE guide](https://docs.vultr.com/platform/iam/oauth/access-tokens/how-to-use-pkce-with-vultr-oauth), [firewall rules](https://docs.vultr.com/products/network/firewall-groups/management/rules) | Confirm a public native-client token exchange without a client secret, or review an advanced user-token flow |
| DigitalOcean | [Droplets API](https://docs.digitalocean.com/reference/api/reference/droplets/), [OAuth](https://docs.digitalocean.com/reference/api/oauth/), [scopes](https://docs.digitalocean.com/reference/api/scopes/), [sizes](https://docs.digitalocean.com/reference/api/reference/sizes/), [firewalls](https://docs.digitalocean.com/reference/api/reference/firewalls/) | Validate desktop/Android redirects, token lifecycle, async creation, exact scopes, pricing and firewall attachment order |

## Acceptance before enabling provider provisioning

Use fake transports/provider fixtures for auth expiry, denied scopes, polling,
partial failures, interrupted installs, duplicate/resumed creates and rollback.
Then run the complete authorization-to-SSH-to-invite flow on a disposable host
for each supported provider/client. Verify billing confirmation and deletion
separately, plus referral attribution where applicable. Record build, environment,
commands and evidence; provider documentation establishes design input, not a pass.
