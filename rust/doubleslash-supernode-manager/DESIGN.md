# Supernode manager implementation

The manager is an independent Rust workspace whose libraries are consumed by its
CLI/TUI and desktop `supernode_admin::AdminService`. It treats the supernode
as an external executable with a manifest, isolated data directory, known ports
and systemd unit. It does not import the application's identity/trust code.

Use the [operator guide](../../docs/supernode-manager.md) for commands, inventory,
installation, updates, clusters and desktop administration. Read
[agents.md](agents.md) before changing this workspace; it owns the host contract.
Open delivery and acceptance work belongs in the
[backlog](../../backlog.md#host-a-supernode-from-the-client).

## Crate boundaries

| Crate | Responsibility |
|---|---|
| `snm-core` | Inventory parsing/resolution, selectors, network and supernode defaults |
| `snm-transport` | Transport trait, embedded russh/OpenSSH, pinned session auth and host-key handling |
| `snm-supernode` | Release/local builds, notices, install/config/lifecycle, systemd/firewall, cluster cache/provisioning, guided setup, hardening, discovery and updater |
| `snm-cli` | Clap commands, ratatui TUI, async workers and fleet presentation |

`snm-cli/src/main.rs` is the command inventory. TUI state, rendering and
workers live under `snm-cli/src/tui/`. Library operations accept transports
and return reports instead of depending on either UI. Desktop adds profile
storage, operation IDs/progress and per-node scheduling in the client workspace.
Android does not yet embed that administration surface.

## State and execution

The CLI holds desired fleet configuration in `inventory.toml` and collected
cluster identities in adjacent `cluster_cache.toml`. Embedded desktop
administration instead stores its managed nodes and SSH credentials in the
profile's encrypted `managed_supernodes.dat`, excluded from backups.
Passwords remain transient. Neither store is a first-party identity backend.

Each remote instance has a data directory, templated systemd unit/drop-in and
its own relay/signaling ports. A cluster adds a member-restricted UDP link.
Instances share the install root's selected binary. Node identity, peer state,
endpoint mailbox and invite remain persistent; SFU room hosting and participant
payloads do not become disk state. There is no `sfu_rooms.json`.

CLI/TUI work runs asynchronously. Client callers must not block a connection
manager loop waiting for an event that loop processes. Fake transports cover
rendering and command failure paths without mutating real servers.

## Operations and invariants

- Install stages notices and the binary, renders manifest/unit/drop-in files,
  applies firewall rules and enables/restarts the unit. It does not provide the
  updater's post-promotion health/rollback guarantee.
- Config push re-renders from inventory and the supplied cached roster. Missing
  roster state must not silently decluster a deployed member. Explicit
  `--allow-decluster` is the override; `cluster-sync` rebuilds rosters/cache.
- Cluster sync collects identities first, renders a full including-self roster,
  updates restricted firewall rules and restarts members. Ordinary redeploy must
  preserve those restricted rules. Private participant allowed sets are not
  replicated across nodes.
- Guided setup sequences probe, key installation, selected hardening, install,
  updater and optional SSH lockdown. Key-only login must succeed before removing
  password login; the user's pinned host key remains authoritative.
- Existing-deployment discovery reads metadata and public fields only. Import
  itself performs no remote mutation. Hostname-only editing later validates
  deployment identity/paths/roster and concurrent manifest changes, preserves
  owner/mode and restarts the exact instance.
- The updater uses package hash/notices, a lock, uniquely named builds and a
  previous symlink. It restarts/checks active or failed instances and preserves
  stopped services. A failed health check rolls back; start limits are cleared
  for restart/rollback. Server-timer mode needs outbound GitHub reachability;
  a client-pushed package does not.
- SHA-256 sidecars are integrity checks against the published package. Manager
  verification of the project's signed release manifest remains unimplemented.

## Validation

Run formatting, focused tests and strict Clippy from this workspace. Updater
shell regressions run on Linux with fake systemd and no root/network:

```sh
python3 -m unittest discover -s tests -p test_autoupdate.py -v
```

Remote acceptance additionally needs a disposable Linux/systemd host, identity
preservation across redeploy, failed update/rollback, client close/reopen without
a node restart, restricted cluster rules and membership changes. The recorded
Debian run and remaining packaged-UI/Ubuntu/import/timer checks are in the
backlog; a source change or historical fleet name is not a current deployment
report.
