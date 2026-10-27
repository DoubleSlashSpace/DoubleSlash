# Guided supernode deployment and administration

## Summary

Add a first-run **Deploy my supernode** path and a persistent Supernode Admin panel to desktop and Android. Both manage an existing Linux systemd host over SSH, then connect the client through the node’s signed invite. Setup also offers an optional Vultr referral link for users who need a server. Future DigitalOcean and Vultr adapters can provision a host through their cloud APIs and hand it to the same SSH deployment flow; DigitalOcean documents a native-client OAuth path, while Vultr's requires confirmation.

## Shared admin service

- Extract the standalone manager’s inventory and operations into a headless Rust service shared by its CLI/TUI and both clients. Expose request IDs, progress, results, and errors without blocking a UI or connection manager loop.
- Provide host and instance editing, connection probe, install, update, start, stop, restart, status, logs, invite, configuration and feature editing, config push, cluster sync, remote command, inventory removal, and confirmed uninstall or purge. Desktop retains compile-from-source deployment; Android deploys verified Linux release packages.
- Keep nonsecret inventory per client profile and allow importing existing `inventory.toml` on either device. SSH credentials are set up separately on each device. The desktop keeps them in `managed_supernodes.dat`, encrypted under an identity-derived store key (like the other profile stores) and excluded from backups; passwords are never stored. Confirm a new host fingerprint and reject a changed key.
- Make Update stage a uniquely named binary and retain the prior binary link and configuration. If service health fails after restart, restore them and report the rollback result. Preserve node identity and persistent peer state.

## Responsibility notice

Before the first deploy, the wizard shows this notice and requires the user to accept it. Acceptance is stored per profile, and the admin panel links back to the notice:

> DoubleSlash can automate initial setup of your server: installing the supernode, configuring a firewall, enabling automatic security updates, and restricting SSH to key login. This is a starting point, not a managed service. You remain solely responsible for the server, including its security, operating-system and software updates, backups, hosting costs, legal compliance where you live and where it runs, and how it is used. Nightly supernode builds are pre-release and may break or change without notice.

## Hardening, access, and update choices

These are implemented in the manager library (`snm-supernode`: `harden.rs`, `ssh_access.rs`, `autoupdate.rs`, `guided.rs`). The wizard shows each choice with its default:

- **SSH access**, chosen in the wizard:
  - *Client key, password login off* (default): the profile's own ed25519 key is installed with the one-time password. A fresh key-only login must succeed, then password login is turned off as the last step. The user is told that losing the profile key means using the provider's console.
  - *Client key, password left on*: the same key, with sshd unchanged.
  - *Own key*: the user's `~/.ssh` key; nothing is installed or changed.

  The password is used for setup only and never stored. Every mode pins the host key the user confirmed by fingerprint.
- **Hardening** (Debian 11+/Ubuntu 20.04+), each step shown and individually optional:
  - a ufw default-deny firewall with SSH rate-limited on the port sshd actually uses;
  - unattended security upgrades, with an optional reboot at 04:00 server time;
  - fail2ban for SSH.

  The probe lists other public listeners the firewall would block. The node's unit is always sandboxed (no capabilities, read-only system, writable data root only).
- **Supernode updates**, chosen in the wizard:
  - *On the server* (default): a daily timer.
  - *From DoubleSlash*: only when the user chooses Update.

  Both run the same on-server updater. It verifies the SHA-256 sidecar, keeps the previous build, and rolls back when an instance does not stay up. The admin panel can switch modes and run Update now in either one.
- **Clustering** is in the first release: the admin panel can add another server to an existing managed node. The wizard deploys the new host, then runs `cluster_sync_report_with` using the client's own transports, which restricts the cluster port to member addresses.

## Setup and provider choice

- After identity setup, both clients offer **Join by invite**, **Deploy my node**, and **Skip**. Deploy collects SSH access and public address, checks the host, previews saved ports and configuration, installs, verifies service health, fetches the reusable invite, and enters the existing signed invite flow. Show success only when the expected node identity connects live; keep failed steps resumable.
- Add a **Need a server?** choice beside **I have a server** in the wizard and admin panel. Its initial catalog has one entry, **Vultr**, opening the exact user supplied URL: `https://www.vultr.com/?ref=6956347`. Put “DoubleSlash may earn a commission if you sign up or purchase through this link” beside the action, following the [FTC’s affiliate disclosure guidance](https://www.ftc.gov/business-guidance/resources/ftcs-endorsement-guides-what-people-are-asking). Open the provider site in the system browser or Android Custom Tab; return to the same SSH setup step when the user has a host. Show no unverified price or performance claims.
- Define a provider catalog with an external signup capability now and separate provisioning adapters for later. The initial signup catalog has the supplied Vultr link; no DigitalOcean affiliate link has been supplied, so do not describe a DigitalOcean link as sponsored or imply it earns a commission. A future adapter returns an SSH target and public host to the existing deployment flow; the initial release does not collect cloud account credentials or call provider APIs. Verify the supplied Vultr referral link and attribution manually before release. Opening a referral link and creating an instance through the API are separate actions; do not assume API provisioning earns referral credit.

## Shared cloud-provider adapter contract

- Keep provider account authorization, resource discovery, provisioning, cloud firewall changes, and provider instance IDs in provider-specific adapters. Feed the resulting SSH target, public address, and detected architecture into the shared admin service. Node install, update, rollback, invite retrieval, and client connection stay provider-independent.
- Both adapters must show live region/image/size availability and current provider prices, then request explicit confirmation immediately before a billable create. Store the provider resource ID and operation progress before moving to the SSH phase so retries can resume without silently creating a second server. A failed node install must leave the still-billable cloud resource visible with a separately confirmed destroy action.
- Request only the API permissions needed for the chosen flow and store each device's tokens in OS protected storage, apart from inventory, logs, invites, and backups. Preserve the manual **I have a server** path. Do not make a cloud account, provider API, or central DoubleSlash backend mandatory.

## Future Vultr provisioning adapter

- **Feasibility:** Vultr documents API creation of Linux cloud instances with region, plan, operating system, SSH keys, cloud-init user data, and a firewall group. An instance exposes its public address and lifecycle status. This can supply the manager's Linux/systemd SSH host contract; it does not replace the manager's install, update, rollback, or invite logic. See [instance provisioning](https://docs.vultr.com/products/compute/instances/cloud-compute/provisioning), [cloud-init user data](https://docs.vultr.com/how-to-deploy-a-vultr-server-with-cloudinit-userdata/), and the [instance resource fields](https://docs.vultr.com/reference/terraform/resources/instance).
- **Adapter contract:** Obtain available regions, compatible Linux images, and plans; show the provider's current price and estimated ongoing charge at a final review; create or select an SSH public key and a firewall group; create the instance; persist its provider ID locally; poll until a public IP is assigned and SSH is ready; return `public_host`, SSH target, architecture, and provider metadata to the shared admin service. Require explicit user confirmation before a billable create request. Never infer that an API success response means the node is deployed or connected.
- **Bootstrap and firewall:** Use Vultr SSH-key attachment or minimal cloud-init to prepare an admin account, then use the existing SSH manager to install and operate the supernode. The cloud firewall must allow the selected SSH source, signaling TCP (default 34935), and relay UDP (default 3478). For a cluster, allow its UDP link port (default 4478) only from sibling node addresses. Configure both Vultr firewall rules and the host's firewall; do not open a public HTTP port for the in-app portal. See [SSH keys](https://docs.vultr.com/products/orchestration/ssh-keys/add-ssh-keys) and [firewall rules](https://docs.vultr.com/products/network/firewall-groups/management/rules).
- **Finish and recovery:** After SSH deployment, verify service status, read `reusable_invite.json` through the manager, enter the signed invite flow, and finish only when the expected node identity connects live. Preserve the provider instance ID and deployment progress so a failed step can resume. Explain that a created instance may continue incurring charges after deployment failure; offer an explicit, separately confirmed destroy action rather than deleting it automatically. Existing nodes and their identity/data directories must not be reprovisioned during updates.
- **Authentication decision:** Vultr offers scoped OAuth tokens, but its [PKCE guide](https://docs.vultr.com/platform/iam/oauth/access-tokens/how-to-use-pkce-with-vultr-oauth) describes mobile/public clients while still showing a client secret at token exchange. Confirm with Vultr, or validate against a test application, that a secure public native-client flow is supported before committing to one-tap desktop or Android account connection. Do not embed an application secret in either client. A user-supplied API key can be considered as an advanced, device-local option after reviewing its permissions and IP access controls; Vultr's [API-key scope FAQ](https://docs.vultr.com/support/platform/api/can-i-scope-api-keys-to-specific-products) describes ordinary keys as having broad account access. Keep cloud credentials out of inventory, logs, invites, and backups.
- **Provider boundary:** The adapter provisions and identifies cloud resources; all DoubleSlash node operations continue through the shared SSH admin service. Keep the manual **I have a server** path available on both clients. The adapter is future work until authentication, pricing display, firewall behavior, and a live disposable-instance deployment are validated.

## Future DigitalOcean provisioning adapter

- **Feasibility:** The [Droplets API](https://docs.digitalocean.com/reference/api/reference/droplets/) creates a Linux Droplet from a region, size, and image, with SSH keys and optional cloud-init `user_data`. It returns a Droplet ID and later exposes status and a public IP. The create response is `202 Accepted`, so poll the create action and Droplet until provisioning succeeds and SSH is ready. Then pass the host to the existing SSH manager and connect through its signed supernode invite.
- **Account connection:** DigitalOcean [documents public OAuth clients](https://docs.digitalocean.com/reference/api/oauth/) with dynamic registration, authorization code plus S256 PKCE, refresh tokens, and no embedded client secret. Use the system browser, `state`, PKCE, and a verified redirect back to the client; protect rotating tokens locally and support disconnect/revocation. Desktop can use a loopback callback with an ephemeral port. Validate the supported callback and return flow on Android before promising in-app account connection there. A user-generated token with custom scopes may be an advanced fallback.
- **Permissions and pricing:** Request granular scopes for reading regions, sizes, images, and Droplets; creating and reading Droplets; and managing only the SSH keys, tags, and firewalls the wizard needs. Request `droplet:delete` only if the user enables provider-side deletion. DigitalOcean's [scope list](https://docs.digitalocean.com/reference/api/scopes/) documents dependencies, and its [sizes endpoint](https://docs.digitalocean.com/reference/api/reference/sizes/) exposes hourly and monthly prices and region availability. Display the provider's current figures and any selected extras before the billable confirmation.
- **Network setup:** Create and associate a [Cloud Firewall](https://docs.digitalocean.com/reference/api/reference/firewalls/) with SSH restricted to the intended admin source, signaling TCP, and relay UDP. Restrict the optional cluster UDP port to sibling nodes. Plan the order of firewall attachment and Droplet creation so a new node is not unintentionally exposed before rules apply; still configure the host firewall through the manager. Do not open a public HTTP port for the in-app portal.
- **Validation:** Mock asynchronous create/action polling, duplicate retries, token expiry and refresh, denied scopes, firewall association, and partial failure. Then test desktop and Android authorization and the complete provision-to-invite flow with a disposable Droplet. The API documentation establishes feasibility, not completed integration or live acceptance.

## Defaults and verification

- Use nightly for guided installs until a stable release is published, then prefer the latest stable release. Pin the channel for each node. Updates follow the mode chosen in the wizard, and always verify the package against its SHA-256 sidecar. In the manual host path, show cloud firewall setup as a visible prerequisite; a future provider adapter configures it before SSH deployment.
- Test shared operations with fake SSH transport, especially host-key changes, interrupted installs, failed updates and rollback, inventory import, and cluster preservation. Test both onboarding flows, provider disclosure and link opening, retries, confirmations, and matching the invite identity to the live connection.
- For future provider adapters, test mocked provision/poll/firewall failures, duplicate or resumed creates, credential loss, and billing confirmation. Then validate the full provision-to-invite flow on disposable Vultr and DigitalOcean instances and check Vultr referral attribution separately; API documentation alone does not establish those live results.
- Run formatting, focused tests, Clippy, Qt integration build, and Android target checks for affected workspaces. Validate deploy, update, rollback, and reconnect on a disposable Linux systemd host from both clients, recording live acceptance separately from automated results.
