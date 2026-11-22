# Supernode manager

The manager deploys and operates `doubleslash-supernode` over SSH. Its
independent Rust workspace provides a CLI/TUI and libraries embedded by desktop
`supernode_admin::AdminService`. Remote install targets are Linux with systemd;
Windows and macOS service backends are not implemented. The manager does not
create client identities or establish contact trust. It reads the node's reusable
invite; the client still performs the signed invite flow.

This is the operator guide. The workspace [agent contract](../rust/doubleslash-supernode-manager/agents.md)
constrains changes, and [DESIGN.md](../rust/doubleslash-supernode-manager/DESIGN.md)
maps the implementation. Live acceptance gaps are in the
[backlog](../backlog.md#host-a-supernode-from-the-client).

## Build and start

From the repository root:

```powershell
Set-Location rust/doubleslash-supernode-manager
cargo build --release
.\launch.ps1
```

No subcommand opens the TUI. `launch.ps1` loads the optional gitignored
`secrets.local.ps1` and runs the CLI/TUI. The executable is
`supernode-manager`. Use `--inventory <path>` to select a fleet; otherwise
it looks for `inventory.toml` in the working directory and above the executable.

## Inventory and SSH

`init` scaffolds inventory and a secrets template. A minimal explicit-port
inventory is:

```toml
[defaults]
version = "nightly"
user = "doubleslash"
install_root = "/opt/doubleslash"
data_root = "/var/lib/doubleslash"
release_repo = "DoubleSlashSpace/DoubleSlash"
privilege = "root" # sudo also works; rootless-systemd is a stub
firewall = "ufw"   # report or off also accepted

[defaults.supernode]
listen_bind = "0.0.0.0"
allow_public_rooms = false
allow_private_rooms = true

[[host]]
name = "east"
ssh = "root@192.0.2.10"

[[host.instance]]
id = "a"
public_host = "192.0.2.10"
relay_port = 3478
ws_port = 34935
```

`(host.name, instance.id)` identifies one deployment. Omitted ports are
allocated at resolution time with a 100-port step; pin ports for stable external
rules. Optional per-instance settings include hostname/lookup mode, bind, access
mode, room policy and feature entries. Features can be strings or objects with
`id`, `enabled` and `params`. See
[inventory.rs](../rust/doubleslash-supernode-manager/crates/snm-core/src/inventory.rs)
and [supernode_config.rs](../rust/doubleslash-supernode-manager/crates/snm-core/src/supernode_config.rs).

Embedded SSH (russh) is default; `--ssh-backend openssh` delegates to system
SSH/scp. Embedded auth tries local keys, host-specific/global password variables,
then interactive auth. `SNM_SSH_PASSWORD_<HOST>` / `SNM_SSH_USER_<HOST>`
use the uppercase inventory name with non-alphanumeric characters replaced by
underscores. Host-specific values override global values. Passwords cached for
the process are scoped to the SSH target. OpenSSH uses its own authentication;
only the user override is passed through. Known-host checks remain enabled;
embedded desktop sessions require the explicitly pinned host key.

## Command surface

```text
supernode-manager [--inventory <path>] [--ssh-backend embedded|openssh] [command]

init [--force]
connect | ping | status | start | stop | restart
install [--allow-decluster]
config-push [--no-restart] [--allow-decluster]
logs [-f] [-n <lines>]
invite
exec <remote-shell-command>
remove [--yes]
uninstall [--purge] [--yes]
cluster-sync [--cluster <id>]
build-deploy --source <package-dir> [--target <triple>]
             [--build-tool cargo|zigbuild|cross] [--features <features>]
             [--allow-decluster]
tui
```

Host-scoped commands accept `--host`, `--instance` and `--all`.
`exec` uses the first match. `remove` edits local inventory only;
`uninstall` stops/disables the remote service, and `--purge` also deletes
its data. The TUI offers fleet editing, room/feature configuration, lifecycle,
build/deploy, cluster sync, logs and invite views. CLI `build-deploy` requires
`--source` even if `defaults.build_source` is configured; the TUI reads
that default. Build artifact discovery honors Cargo target directories and
cross-target executable suffixes.

There is no CLI `update` or `plan` command. The safe updater described
below is a library/desktop flow, not the same operation as reinstalling.

## Host contract

Each instance gets `{data_root}/{id}` through `DOUBLESLASH_HOME`, a
`doubleslash-supernode@{id}.service` unit and a per-instance systemd drop-in.
Instances share `{install_root}/bin/current` pointing to a versioned binary.
Install uploads distribution notices before promoting the executable, renders
manifest/unit/drop-in files, reloads systemd, applies selected firewall rules,
enables the unit and restarts it. It preserves identity, peer state and the
endpoint mailbox. Reinstall is not a rollback-checked update.

| State | Ownership / lifetime |
|---|---|
| `identity.json`, `peers.json`, `supernode_endpoints.json` | Node persistent state; preserve across redeploy |
| `reusable_invite.json` | Node invite, read through the manager |
| `supernode.toml` | Rendered configuration; runtime applies bind/access/feature/cluster settings |
| `identity_file` inventory/manifest field | Descriptive only; runtime still loads `identity.json` |
| `web/`, `games/` | Operator assets and seeded built-in portal apps |
| SFU room definitions and participant payloads | Memory-only hosting; client-owned definitions/history; no `sfu_rooms.json` |

Configuration and plugin changes require restart. The unit has no capabilities,
read-only system paths and a writable data root. New runtime writes outside that
root require a corresponding unit change. `root` and `sudo` privilege
modes work; `rootless-systemd` install/uninstall does not.

Public listeners are relay UDP (default 3478) and signaling TCP (34935). Cluster
UDP (4478 base) is separate and member-restricted. There is no public web/game
port. `ufw` mode manages tagged rules; `report` lists ports and `off`
skips mutation. Host firewalls do not configure a provider's cloud firewall.

Status reports systemd state and the selected binary's SHA-256, mtime and optional
embedded build ID. The version pin alone does not identify the installed bytes.
Logs use journalctl; the TUI fetches snapshots, while CLI `logs -f` follows.
No public HTTP metrics scraper is implemented.

## Clusters and redeployment

Declare member instance keys in inventory:

```toml
[[cluster]]
id = "team"
members = ["east/a", "west/a"] # both hosts/instances must exist in inventory
cluster_port = 4478
```

Install each member first to create its node identity, then run:

```powershell
.\launch.ps1 cluster-sync --cluster team
.\launch.ps1 status --all
```

`cluster-sync` collects public identities, renders the full roster including
self into every manifest, updates the local `cluster_cache.toml`, restricts
cluster firewall rules to member addresses and restarts members. Nodes
authenticate cluster links against the roster and sign control messages. A
verified roster lets clients fail over to siblings. Replication carries room
existence and room certificates, Space roots, peer trust and opaque room/game
traffic, not private-room allowed sets or participant-content storage.

Install/config-push/build-deploy resolve rosters from inventory membership and
the local cache. They refuse to replace a deployed cluster section with no
roster unless `--allow-decluster` is explicit. Keep the inventory and cache;
run `cluster-sync` if the cache is missing or membership changes. Do not
assume the manager copies arbitrary remote manifest edits back into inventory.
Redeploy preserves member-restricted firewall rules; only cluster sync refreshes
those rules. Uninstall removes the instance's rules.

After redeploy, inspect each member's roster, binary hash, service state and
client proof/reconnect logs. A fixed count of lines containing "cluster" is not
an acceptance check. Do not restart the whole cluster when testing a client
close/reopen: that can hide stale-session bugs.

## Release download and updates

Release installs resolve Linux x86_64/aarch64 tarballs, verify the required
SHA-256 sidecar and retain/upload `licenses/`. Missing notices fail the
install. Local binaries need adjacent `<binary>.licenses/`; `build-deploy`
generates it using Node.js and cargo-about 0.8.4. Windows zip extraction and
project Ed25519 release-manifest verification are not implemented in the manager.

The library updater supports client-pushed packages and a server timer. Both
verify package hashes/notices, use a lock, stage a uniquely named build, retain
the previous link and check service health after promotion. Active, activating,
reloading and failed instances are restarted and checked; deliberately inactive
services remain stopped. Restart and rollback clear systemd start limits. Failure
to list units aborts promotion. A failed health check restores the previous binary;
data directories are untouched. Update now refreshes the updater script even
when the binary is already current.

Server-timer mode is daily with a randomized delay and requires server access to
GitHub. Client-driven checks/downloads/push work for hosts without that access.
The guided probe refuses timer mode when GitHub is unreachable. See
[autoupdate.rs](../rust/doubleslash-supernode-manager/crates/snm-supernode/src/autoupdate.rs)
and the Linux fake-systemd regression suite:

```sh
# From rust/doubleslash-supernode-manager on Linux
python3 -m unittest discover -s tests -p test_autoupdate.py -v
```

## Desktop administration

Open **Settings > Network > Your servers**. New setup accepts a Linux SSH host,
shows its fingerprint and responsibility notice, probes it and offers SSH-key,
firewall, OS-security-update and supernode-update choices. Client-key mode must
verify a fresh key-only login before optional password-login lockdown. Passwords
are transient; the profile's SSH key, pinned hosts and managed metadata are in
encrypted `managed_supernodes.dat`, excluded from portable backups.

Each node groups its servers. Controls inspect status/logs, copy its invite,
start/restart, check/push updates, select update mode, rename locally, add/remove
cluster members, sync the roster, uninstall or forget. Different nodes can run
operations concurrently; one node serializes its tasks. Start requires SSH,
clears failed/start-limit state and starts the service; it cannot power on or
repair an unresponsive VPS. Provider recovery is separate.

**Import existing** is read-only discovery over pinned SSH. It needs Linux
systemd units named `doubleslash-supernode@<id>.service`, Python 3, root or
passwordless sudo, an absolute `DOUBLESLASH_HOME` and schema-1 manifest.
Select instances from a public-only preview. Import changes no remote files,
SSH keys, firewall or services and does not accept an invite. Import each VPS
separately; matching normalized cluster rosters group instances. Other members
remain informational until imported. Imported deployments retain their existing
deploy/update/lifecycle/cluster tools; Forget removes the local entry only.

**Set hostname** supports DNS or DDDNS and shows SSH-discovered IPv4/IPv6 route
sources plus signaling/relay ports. Refresh is read-only, not an external
reachability/NAT test. Managed setup/Apply upgrades wildcard binds to dual stack
where available and preserves explicit binds. Imported Apply re-checks identity,
paths, roster and manifest hash, edits only hostname fields, atomically preserves
owner/mode and restarts exactly that instance; split binds/custom features remain.
Changed, linked or shared manifests are refused. Password SSH requires re-entry
for Apply/Refresh.

For DDDNS, review the displayed HTTPS update endpoint and provide a masked,
one-use token. After SSH configuration succeeds, the desktop publishes both
address families and the signaling port, explicitly clearing missing families
and excluding IPv6 on IPv4-only signaling. A matching service reply confirms
publication. A failed publication is reported separately and can be retried;
setup success does not imply publication success. Tokens are never saved or
installed on the server. Applying a name to a different server replaces its
record; clearing local hostname configuration does not delete the record. No
continuous updater is installed. See [DDDNS](DDDNS.md) and [privacy](../PRIVACY.md).

## Validation and open work

A library-driven IPv6-only Debian 12 run on 2026-10-27 exercised setup,
key/lockdown, client-pushed update and forced rollback. It did not validate the
packaged wizard, timer mode or all cluster/import flows. Current pending checks,
including Start, Ubuntu, IPv6-only relay and a reported small-VPS outage, live in
the [backlog](../backlog.md#host-a-supernode-from-the-client).

Android administration, inventory-file import, desktop feature/policy editing,
provider API provisioning, CLI plan/update, rootless services, signed release
manifests, audit logging and remote non-Linux supervisors remain unfinished.
The [administration plan](SUPERNODE_CLIENT_ADMIN_PLAN.md) covers deferred client
and provider work. Do not infer a live fleet's health from historical test hosts.
