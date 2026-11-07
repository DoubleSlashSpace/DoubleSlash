//! JSON commands for the supernode admin UI, run on the client's runtime.
//!
//! Each request names an `op`. Long operations report progress through
//! `emit` as `{"kind":"plan"|"step", …}` events; every request ends with one
//! result value or an error string. Nothing here touches Qt, so the same
//! service can back the Android client later.

use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use snm_core::{ClusterDef, Inventory, PrivilegeMode, ResolvedInstance, Selector};
use snm_supernode::{
    cluster_sync_report_with, download_release_archive, guided_steps, installed_package,
    invite_instance, lifecycle_report, logs_instance_text, probe_host_os, published_release_sha256,
    push_config_instance_report, push_update, run_guided_setup, set_update_mode, status_instance,
    uninstall_instance_report, ClusterCache, GuidedProgress, GuidedSetup, HardeningOptions,
    LifecycleAction, OsSupport, SshAccessMode, StepState, UpdateMode,
};
use snm_transport::{
    fetch_host_key, host_key_fingerprint, parse_public_key, PrivateKey, PublicKey, SessionAuth,
    SshTransport,
};
use zeroize::Zeroize;

use super::dddns::{UpdatePlan, UpdateRequest};
use super::network::{probe_addresses, probe_listen_bind, PublicAddresses};
use super::store::{ManagedHost, ManagedNode, ManagedNodeStore};
use crate::peer_store::clean_local_name;

/// Shown, and accepted, before the first deploy.
pub const RESPONSIBILITY_NOTICE: &str = "DoubleSlash can automate initial setup of your server: \
installing the supernode, configuring a firewall, enabling automatic security updates, and \
restricting SSH to key login. This is a starting point, not a managed service. You remain solely \
responsible for the server, including its security, operating-system and software updates, \
backups, hosting costs, legal compliance where you live and where it runs, and how it is used. \
Nightly supernode builds are pre-release and may break or change without notice.";

/// The only channel offered until a stable release is published.
const CHANNEL: &str = "nightly";
const KEY_COMMENT: &str = "doubleslash-desktop";
const LINK_STEP: &str = "Link the servers into one node";

pub type Emit<'a> = &'a (dyn Fn(Value) + Send + Sync);

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    State,
    AcceptNotice,
    FetchHostKey {
        host: String,
        #[serde(default = "default_port")]
        port: u16,
    },
    Deploy(ServerRequest),
    AddServer {
        node_id: String,
        #[serde(flatten)]
        server: ServerRequest,
    },
    Status {
        node_id: String,
        #[serde(default)]
        host_name: Option<String>,
    },
    ConnectionInfo {
        node_id: String,
        host_name: String,
    },
    Logs {
        node_id: String,
        #[serde(default)]
        host_name: Option<String>,
        #[serde(default)]
        lines: Option<u32>,
    },
    /// Read the node's current reusable invite from a server. This is the
    /// link other people accept to add the supernode. A room invite is not.
    Invite {
        node_id: String,
    },
    Restart {
        node_id: String,
        #[serde(default)]
        host_name: Option<String>,
    },
    CheckUpdate {
        node_id: String,
        #[serde(default)]
        host_name: Option<String>,
    },
    UpdateNow {
        node_id: String,
        #[serde(default)]
        host_name: Option<String>,
    },
    SetUpdateMode {
        node_id: String,
        mode: String,
    },
    SetHostname {
        node_id: String,
        host_name: String,
        #[serde(flatten)]
        settings: HostnameSettings,
        #[serde(flatten)]
        update: UpdateRequest,
    },
    Uninstall {
        node_id: String,
        #[serde(default)]
        purge: bool,
    },
    Forget {
        node_id: String,
    },
    /// Take one server out of a clustered supernode and relink the rest.
    RemoveServer {
        node_id: String,
        host_name: String,
        /// Uninstall the supernode from that server first. Off drops a server
        /// that can no longer be reached without contacting it.
        uninstall: bool,
        #[serde(default)]
        purge: bool,
    },
    /// Push the current member list to every server of a supernode again.
    SyncCluster {
        node_id: String,
    },
    /// This profile's name for a supernode. Nothing is sent to a server.
    Rename {
        node_id: String,
        name: String,
    },
}

/// What a running request holds. Setups share the release download cache,
/// so one runs at a time; a supernode runs one task at a time; different
/// supernodes run side by side.
#[derive(Debug, PartialEq, Eq)]
struct Claim<'a> {
    setup: bool,
    node: Option<&'a str>,
}

impl Request {
    /// What the request must hold while it runs. Reads and local-only edits
    /// hold nothing.
    fn claim(&self) -> Option<Claim<'_>> {
        match self {
            Self::Deploy(_) => Some(Claim {
                setup: true,
                node: None,
            }),
            Self::AddServer { node_id, .. } => Some(Claim {
                setup: true,
                node: Some(node_id),
            }),
            Self::Restart { node_id, .. }
            | Self::UpdateNow { node_id, .. }
            | Self::SetUpdateMode { node_id, .. }
            | Self::SetHostname { node_id, .. }
            | Self::Uninstall { node_id, .. }
            | Self::Forget { node_id }
            | Self::RemoveServer { node_id, .. }
            | Self::SyncCluster { node_id } => Some(Claim {
                setup: false,
                node: Some(node_id),
            }),
            Self::State
            | Self::AcceptNotice
            | Self::FetchHostKey { .. }
            | Self::Status { .. }
            | Self::ConnectionInfo { .. }
            | Self::Logs { .. }
            | Self::Invite { .. }
            | Self::CheckUpdate { .. }
            | Self::Rename { .. } => None,
        }
    }
}

#[derive(Default)]
struct Tasks {
    setup: bool,
    nodes: BTreeSet<String>,
}

/// Held while a server task runs. Dropping it frees what the task claimed.
pub struct TaskGuard {
    tasks: Arc<Mutex<Tasks>>,
    setup: bool,
    node: Option<String>,
}

impl Drop for TaskGuard {
    fn drop(&mut self) {
        let mut tasks = self.tasks.lock();
        if self.setup {
            tasks.setup = false;
        }
        if let Some(node) = &self.node {
            tasks.nodes.remove(node);
        }
    }
}

/// One server, as entered in the wizard.
#[derive(Deserialize)]
pub struct ServerRequest {
    #[serde(default)]
    name: String,
    host: String,
    #[serde(default = "default_user")]
    user: String,
    #[serde(default = "default_port")]
    port: u16,
    /// Where clients reach the node; the SSH host when empty.
    #[serde(default)]
    public_host: String,
    #[serde(flatten)]
    hostname: HostnameSettings,
    #[serde(flatten)]
    update: UpdateRequest,
    /// The OpenSSH key line the user confirmed by fingerprint.
    host_key: String,
    #[serde(default)]
    password: Option<String>,
    access: String,
    /// Deploy only: a new server in a node follows the node's mode.
    #[serde(default)]
    update_mode: Option<String>,
    #[serde(default)]
    hardening: HardeningRequest,
    #[serde(default)]
    allow_public_rooms: bool,
}

impl Drop for ServerRequest {
    fn drop(&mut self) {
        if let Some(password) = &mut self.password {
            password.zeroize();
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct HardeningRequest {
    firewall: bool,
    automatic_security_updates: bool,
    automatic_reboot: bool,
    fail2ban: bool,
}

impl Default for HardeningRequest {
    fn default() -> Self {
        let d = HardeningOptions::default();
        Self {
            firewall: d.firewall,
            automatic_security_updates: d.automatic_security_updates,
            automatic_reboot: d.automatic_reboot,
            fail2ban: d.fail2ban,
        }
    }
}

fn default_user() -> String {
    "root".into()
}

fn default_port() -> u16 {
    22
}

/// A server request, checked and parsed.
struct CheckedServer {
    ssh_target: String,
    public_host: String,
    hostname: HostnameSettings,
    update_plan: Option<UpdatePlan>,
    host_key: PublicKey,
    access: SshAccessMode,
}

impl ServerRequest {
    fn check(&self) -> Result<CheckedServer, String> {
        let host = ssh_host(&self.host).ok_or(BAD_HOST)?;
        let user = self.user.trim();
        if user.is_empty()
            || !user
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        {
            return Err("Enter a plain login name, such as root".into());
        }
        // Relay tickets carry an address the client parses as a socket
        // address, so the node must advertise an IP literal, not a name.
        let public_ip: IpAddr = match unbracket(self.public_host.trim()) {
            "" => host.parse().map_err(|_| {
                "Enter the server's public IP address: clients reach a supernode by address, \
                 not by name"
                    .to_string()
            })?,
            other => other
                .parse()
                .map_err(|_| "The public address must be an IP address".to_string())?,
        };
        let host_key = parse_public_key(&self.host_key)
            .map_err(|_| "Confirm the server's host key before continuing".to_string())?;
        let access = SshAccessMode::parse(&self.access)
            .ok_or_else(|| format!("Unknown SSH access choice {:?}", self.access))?;
        let hostname = self.hostname.checked()?;
        let update_plan = self
            .update
            .plan(&hostname.hostname, &hostname.hostname_lookup)?;
        Ok(CheckedServer {
            ssh_target: format!("{user}@{}:{}", bracket(host), self.port),
            public_host: match public_ip {
                IpAddr::V4(ip) => ip.to_string(),
                IpAddr::V6(ip) => format!("[{ip}]"),
            },
            hostname,
            update_plan,
            host_key,
            access,
        })
    }

    fn hardening(&self) -> HardeningOptions {
        HardeningOptions {
            firewall: self.hardening.firewall,
            automatic_security_updates: self.hardening.automatic_security_updates,
            automatic_reboot: self.hardening.automatic_security_updates
                && self.hardening.automatic_reboot,
            fail2ban: self.hardening.fail2ban,
        }
    }
}

/// Optional advertised name, separate from the public IP and SSH address.
#[derive(Clone, Default, Deserialize)]
pub struct HostnameSettings {
    #[serde(default)]
    hostname: String,
    /// Empty means choose by name, as the supernode does.
    #[serde(default)]
    hostname_lookup: String,
}

impl HostnameSettings {
    fn checked(&self) -> Result<Self, String> {
        let lookup = self.hostname_lookup.trim().to_ascii_lowercase();
        if !matches!(lookup.as_str(), "" | "dns" | "dddns") {
            return Err("Choose Automatic, DNS or DDDNS for hostname lookup".into());
        }
        let raw = self.hostname.trim();
        if raw.is_empty() {
            return Ok(Self::default());
        }
        // Same validation as the supernode's normalize_advertised_hostname.
        let hostname = crate::connection_fallback::normalize_dns_name(raw)
            .ok_or("Enter a hostname without a scheme, port or path")?;
        if lookup == "dddns" && !crate::connection_fallback::is_dddns_lookup_name(&hostname) {
            return Err("A DDDNS hostname needs a name and zone, such as node.example.com".into());
        }
        Ok(Self {
            hostname,
            hostname_lookup: lookup,
        })
    }

    fn apply(&self, instance: &mut snm_core::Instance) {
        instance.hostname = (!self.hostname.is_empty()).then(|| self.hostname.clone());
        instance.hostname_lookup =
            (!self.hostname_lookup.is_empty()).then(|| self.hostname_lookup.clone());
    }
}

const BAD_HOST: &str = "Enter the server's IP address or host name";

/// The SSH host as typed, without brackets: an IP literal or a DNS name.
fn ssh_host(raw: &str) -> Option<&str> {
    let host = unbracket(raw.trim());
    let dns_name = !host.is_empty()
        && host.len() <= 253
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !host.starts_with('-');
    (dns_name || host.parse::<IpAddr>().is_ok()).then_some(host)
}

fn unbracket(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host)
}

/// IPv6 literals in `host:port` strings need brackets.
fn bracket(host: &str) -> String {
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

pub struct AdminService {
    store: Arc<Mutex<ManagedNodeStore>>,
    tasks: Arc<Mutex<Tasks>>,
    /// Cluster roster cache the manager library keeps beside its inventory.
    cluster_cache: PathBuf,
    /// Every supernode's roster goes through the one cache file, which a sync
    /// reads and rewrites, so syncs take turns.
    cluster_sync: tokio::sync::Mutex<()>,
    /// Serialize edits to one DDDNS record across different managed nodes.
    dddns_changes: Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>,
}

impl AdminService {
    pub fn new(store: ManagedNodeStore, profile_dir: PathBuf) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            tasks: Arc::default(),
            cluster_cache: profile_dir.join("managed_supernodes_cluster_cache.toml"),
            cluster_sync: tokio::sync::Mutex::new(()),
            dddns_changes: Mutex::default(),
        }
    }

    /// Everything the admin panel shows without contacting a server.
    pub fn state(&self) -> Value {
        let (notice_accepted, client_public_key, nodes) = {
            let store = self.store.lock();
            let client_public_key = store
                .existing_client_key()
                .and_then(|k| k.public_key().to_openssh().ok());
            let nodes: Vec<Value> = store.nodes().iter().map(node_json).collect();
            (store.notice_accepted(), client_public_key, nodes)
        };
        let tasks = self.tasks.lock();
        json!({
            "notice": RESPONSIBILITY_NOTICE,
            "notice_accepted": notice_accepted,
            "client_public_key": client_public_key,
            "nodes": nodes,
            "busy_nodes": tasks.nodes.iter().collect::<Vec<_>>(),
            "setup_running": tasks.setup,
        })
    }

    /// Whether any server task is running.
    pub fn has_tasks(&self) -> bool {
        let tasks = self.tasks.lock();
        tasks.setup || !tasks.nodes.is_empty()
    }

    /// Claim what `request` needs before it runs, or say what is in the way.
    /// The caller keeps the guard until the task ends.
    pub fn begin(&self, request: &Request) -> Result<Option<TaskGuard>, String> {
        request
            .claim()
            .map(|claim| self.claim(claim.setup, claim.node))
            .transpose()
    }

    fn claim(&self, setup: bool, node: Option<&str>) -> Result<TaskGuard, String> {
        let mut tasks = self.tasks.lock();
        if setup && tasks.setup {
            return Err("Another server setup is still running".into());
        }
        if node.is_some_and(|id| tasks.nodes.contains(id)) {
            return Err("This supernode is still busy with another task".into());
        }
        tasks.setup |= setup;
        if let Some(id) = node {
            tasks.nodes.insert(id.to_owned());
        }
        Ok(TaskGuard {
            tasks: Arc::clone(&self.tasks),
            setup,
            node: node.map(str::to_owned),
        })
    }

    /// Claim, then run. For callers that do not show the claim first.
    pub async fn handle(&self, request: Request, emit: Emit<'_>) -> Result<Value, String> {
        let task = self.begin(&request)?;
        self.run(request, task, emit).await
    }

    /// Run a request whose claim the caller took with [`Self::begin`]. The
    /// claim is released when this returns.
    pub async fn run(
        &self,
        request: Request,
        _task: Option<TaskGuard>,
        emit: Emit<'_>,
    ) -> Result<Value, String> {
        match request {
            Request::State => Ok(self.state()),
            Request::AcceptNotice => {
                self.store
                    .lock()
                    .accept_notice(now_secs())
                    .map_err(|e| e.to_string())?;
                Ok(self.state())
            }
            Request::FetchHostKey { host, port } => {
                let host = ssh_host(&host).ok_or(BAD_HOST)?;
                let key = fetch_host_key(host, port)
                    .await
                    .map_err(|e| format!("Could not reach {host}:{port}: {e}"))?;
                Ok(json!({
                    "fingerprint": host_key_fingerprint(&key),
                    "host_key": key.to_openssh().map_err(|e| e.to_string())?,
                }))
            }
            Request::Deploy(server) => self.deploy(server, emit).await,
            Request::AddServer { node_id, server } => self.add_server(&node_id, server, emit).await,
            Request::Status { node_id, host_name } => {
                self.status(&node_id, host_name.as_deref()).await
            }
            Request::ConnectionInfo { node_id, host_name } => {
                self.connection_info(&node_id, &host_name).await
            }
            Request::Logs {
                node_id,
                host_name,
                lines,
            } => self.logs(&node_id, host_name.as_deref(), lines).await,
            Request::Invite { node_id } => self.invite(&node_id).await,
            Request::Restart { node_id, host_name } => {
                let lines = self
                    .for_each_host(&node_id, host_name.as_deref(), HostAction::Restart)
                    .await?;
                Ok(json!({ "lines": lines }))
            }
            Request::CheckUpdate { node_id, host_name } => {
                self.check_update(&node_id, host_name.as_deref()).await
            }
            Request::UpdateNow { node_id, host_name } => {
                self.update_now(&node_id, host_name.as_deref()).await
            }
            Request::SetUpdateMode { node_id, mode } => {
                let parsed = UpdateMode::parse(&mode)
                    .ok_or_else(|| format!("Unknown update mode {mode:?}"))?;
                let lines = self
                    .for_each_host(&node_id, None, HostAction::SetUpdateMode(parsed))
                    .await?;
                self.edit_node(&node_id, |node| node.update_mode = mode.clone())?;
                Ok(json!({ "lines": lines, "state": self.state() }))
            }
            Request::SetHostname {
                node_id,
                host_name,
                settings,
                update,
            } => {
                self.set_hostname(&node_id, &host_name, settings, update)
                    .await
            }
            Request::Uninstall { node_id, purge } => {
                let lines = self
                    .for_each_host(&node_id, None, HostAction::Uninstall { purge })
                    .await?;
                self.store
                    .lock()
                    .remove(&node_id)
                    .map_err(|e| e.to_string())?;
                Ok(json!({ "lines": lines, "state": self.state() }))
            }
            Request::Forget { node_id } => {
                self.store
                    .lock()
                    .remove(&node_id)
                    .map_err(|e| e.to_string())?;
                Ok(self.state())
            }
            Request::RemoveServer {
                node_id,
                host_name,
                uninstall,
                purge,
            } => {
                self.remove_server(&node_id, &host_name, uninstall, purge)
                    .await
            }
            Request::SyncCluster { node_id } => self.sync_cluster(&node_id).await,
            Request::Rename { node_id, name } => {
                let name = clean_local_name(&name);
                if name.is_empty() {
                    return Err("Enter a name".into());
                }
                self.edit_node(&node_id, |node| node.name = name)?;
                Ok(self.state())
            }
        }
    }

    fn require_notice(&self) -> Result<(), String> {
        if self.store.lock().notice_accepted() {
            Ok(())
        } else {
            Err("Accept the responsibility notice before setting up a server".into())
        }
    }

    /// The profile key, for the access modes that install it.
    fn key_for(&self, access: SshAccessMode) -> Result<Option<PrivateKey>, String> {
        if !access.installs_client_key() {
            return Ok(None);
        }
        self.store
            .lock()
            .client_key(KEY_COMMENT)
            .map(Some)
            .map_err(|e| e.to_string())
    }

    async fn deploy(&self, server: ServerRequest, emit: Emit<'_>) -> Result<Value, String> {
        self.require_notice()?;
        let checked = server.check()?;
        let _dddns_turn = self.dddns_turn(checked.update_plan.as_ref()).await;
        let update_mode = server
            .update_mode
            .as_deref()
            .and_then(UpdateMode::parse)
            .ok_or("Choose how the supernode is updated")?;
        let client_key = self.key_for(checked.access)?;
        let name = match clean_local_name(&server.name) {
            name if name.is_empty() => checked.public_host.clone(),
            name => name,
        };
        let mut setup = guided_setup(
            &server,
            &checked,
            host_slug(&name, &[]),
            client_key,
            update_mode,
        );
        let probe = SshTransport::with_auth(&checked.ssh_target, setup_auth(&setup));
        setup.instance.listen_bind = Some(probe_listen_bind(&probe).await?);

        // Setting up a server this profile already manages again (to change
        // its choices, or after fixing something) updates that entry rather
        // than listing the server twice. That supernode must not be running
        // another task meanwhile.
        let existing = self
            .store
            .lock()
            .nodes()
            .iter()
            .find(|n| n.hosts.len() == 1 && n.hosts[0].public_host == checked.public_host)
            .map(|n| n.id.clone());
        let _existing_task = existing
            .as_deref()
            .map(|id| self.claim(false, Some(id)))
            .transpose()?;

        let outcome = run_with_progress(&setup, &[], emit).await?;
        let host = managed_host(&setup, &outcome);
        let node = ManagedNode {
            id: existing.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            name,
            channel: CHANNEL.into(),
            update_mode: update_mode.as_str().into(),
            cluster_id: None,
            invite_url: outcome.invite_url.clone(),
            created_at: now_secs(),
            hosts: vec![host],
            inventory: outcome.inventory,
        };
        let node_id = node.id.clone();
        self.store.lock().upsert(node).map_err(|e| e.to_string())?;
        let warning = self
            .publish_after_setup(
                &node_id,
                &setup.host_name,
                checked.update_plan.as_ref(),
                &server.update,
            )
            .await;
        Ok(json!({
            "node_id": node_id,
            "node_identity": outcome.node_identity,
            "accept_invite": outcome.invite_url,
            "ports": ports_note(&setup),
            "state": self.state(),
            "warning": warning,
            "dddns_updated": checked.update_plan.is_some() && warning.is_none(),
        }))
    }

    async fn add_server(
        &self,
        node_id: &str,
        server: ServerRequest,
        emit: Emit<'_>,
    ) -> Result<Value, String> {
        self.require_notice()?;
        let node = self.node(node_id)?;
        let checked = server.check()?;
        let _dddns_turn = self.dddns_turn(checked.update_plan.as_ref()).await;
        if node
            .hosts
            .iter()
            .any(|h| h.public_host == checked.public_host)
        {
            return Err("That server is already part of this node".into());
        }
        let update_mode = UpdateMode::parse(&node.update_mode).unwrap_or(UpdateMode::ServerTimer);
        let client_key = self.key_for(checked.access)?;
        let existing: Vec<&str> = node.hosts.iter().map(|h| h.host_name.as_str()).collect();
        let host_name = host_slug(
            &format!("{}-{}", node.name, node.hosts.len() + 1),
            &existing,
        );
        let mut setup = guided_setup(&server, &checked, host_name, client_key, update_mode);

        // The cluster's shared defaults carry one privilege mode, so a new
        // server must be logged into the same way as the first.
        let probe = SshTransport::with_auth(&checked.ssh_target, setup_auth(&setup));
        setup.instance.listen_bind = Some(probe_listen_bind(&probe).await?);
        let os = probe_host_os(&probe).await.map_err(|e| format!("{e:#}"))?;
        if let OsSupport::Unsupported(reason) = os.support() {
            return Err(reason);
        }
        if os.privilege() != Some(node.inventory.defaults.privilege) {
            return Err(match node.inventory.defaults.privilege {
                PrivilegeMode::Root => "Log in to the new server as root, like the first one",
                _ => {
                    "Log in to the new server as a user with passwordless sudo, like the first one"
                }
            }
            .into());
        }

        let outcome = run_with_progress(&setup, &[LINK_STEP], emit).await?;
        let link_index = guided_steps(&setup).len();
        let step = |state: &str, lines: Vec<String>, error: Option<String>| {
            emit(json!({
                "kind": "step", "index": link_index, "title": LINK_STEP,
                "state": state, "lines": lines, "error": error,
            }))
        };
        step("running", Vec::new(), None);

        let new_host = managed_host(&setup, &outcome);
        let cluster_id = cluster_id_for(&node);
        // Save the server before linking: a failed link must not lose track
        // of a machine that now runs a node.
        let node = self.edit_node(node_id, |node| {
            node.inventory
                .host
                .extend(outcome.inventory.host.iter().cloned());
            node.inventory.clusters = vec![cluster_def(&cluster_id, &node.inventory)];
            node.hosts.push(new_host);
            node.clone()
        })?;

        match self.link_cluster(&node, &cluster_id).await {
            Ok(lines) => {
                step("done", lines, None);
                self.edit_node(node_id, |node| node.cluster_id = Some(cluster_id))?;
                let warning = self
                    .publish_after_setup(
                        node_id,
                        &setup.host_name,
                        checked.update_plan.as_ref(),
                        &server.update,
                    )
                    .await;
                Ok(
                    json!({ "node_id": node_id, "ports": ports_note(&setup), "state": self.state(), "warning": warning,
                        "dddns_updated": checked.update_plan.is_some() && warning.is_none() }),
                )
            }
            Err(message) => {
                step("failed", Vec::new(), Some(message.clone()));
                Err(format!("{LINK_STEP}: {message}"))
            }
        }
    }

    /// Take one server out of a cluster. It is uninstalled first when asked;
    /// then it leaves the record, and the remaining servers get a member list
    /// without it, which also drops it from their cluster firewall rules.
    async fn remove_server(
        &self,
        node_id: &str,
        host_name: &str,
        uninstall: bool,
        purge: bool,
    ) -> Result<Value, String> {
        let node = self.node(node_id)?;
        let host = node
            .host(host_name)
            .ok_or_else(|| format!("No such server in this node: {host_name}"))?;
        if node.hosts.len() < 2 {
            return Err(
                "This is the supernode's only server. Remove the supernode instead.".into(),
            );
        }
        let mut lines = Vec::new();
        if uninstall {
            let key = self.store.lock().existing_client_key();
            let transport = host_transport(host, key.as_ref())?;
            let resolved = resolve_host(&node.inventory, host_name)?;
            let removed = run_host_action(HostAction::Uninstall { purge }, &transport, &resolved)
                .await
                .map_err(|e| format!("{host_name}: {e}"))?;
            lines.extend(removed.into_iter().map(|l| format!("{host_name}: {l}")));
        }

        let cluster_id = cluster_id_for(&node);
        // Forget the server before relinking, so a failed relink cannot leave
        // a server listed that may already be gone. Sync cluster retries.
        let node = self.edit_node(node_id, |node| {
            node.hosts.retain(|h| h.host_name != host_name);
            node.inventory.host.retain(|h| h.name != host_name);
            node.inventory.clusters = vec![cluster_def(&cluster_id, &node.inventory)];
            node.clone()
        })?;
        let relinked = self.link_cluster(&node, &cluster_id).await.map_err(|e| {
            format!(
                "{host_name} was removed, but the other servers still list it: {e}. \
                 Sync the cluster to try again."
            )
        })?;
        lines.extend(relinked);
        self.edit_node(node_id, |node| node.cluster_id = Some(cluster_id))?;
        Ok(json!({ "lines": lines, "state": self.state() }))
    }

    /// Push the member list to every server again: after a failed link or
    /// relink, or when a server was restored from elsewhere.
    async fn sync_cluster(&self, node_id: &str) -> Result<Value, String> {
        let node = self.node(node_id)?;
        if node.hosts.len() < 2 && node.cluster_id.is_none() {
            return Err("This supernode has one server. Add a cluster member first.".into());
        }
        let cluster_id = cluster_id_for(&node);
        let node = self.edit_node(node_id, |node| {
            node.inventory.clusters = vec![cluster_def(&cluster_id, &node.inventory)];
            node.clone()
        })?;
        let lines = self.link_cluster(&node, &cluster_id).await?;
        self.edit_node(node_id, |node| node.cluster_id = Some(cluster_id))?;
        Ok(json!({ "lines": lines, "state": self.state() }))
    }

    /// Give each of the node's servers the shared member list and restart it,
    /// one server at a time.
    async fn link_cluster(
        &self,
        node: &ManagedNode,
        cluster_id: &str,
    ) -> Result<Vec<String>, String> {
        let cluster = cluster_def(cluster_id, &node.inventory);
        let key = self.store.lock().existing_client_key();
        let _turn = self.cluster_sync.lock().await;
        cluster_sync_report_with(&node.inventory, &cluster, &self.cluster_cache, |host| {
            let managed = node
                .host(&host.name)
                .ok_or_else(|| anyhow::anyhow!("no credentials for {}", host.name))?;
            host_transport(managed, key.as_ref()).map_err(anyhow::Error::msg)
        })
        .await
        .map_err(|e| format!("{e:#}"))
    }

    /// Read-only, so a cluster's servers are asked together.
    async fn status(&self, node_id: &str, host_name: Option<&str>) -> Result<Value, String> {
        let node = self.node(node_id)?;
        let key = self.store.lock().existing_client_key();
        let checks = selected_hosts(&node, host_name)?.into_iter().map(|host| {
            let key = key.as_ref();
            let inventory = &node.inventory;
            async move {
                match (
                    host_transport(host, key),
                    resolve_host(inventory, &host.host_name),
                ) {
                    (Ok(transport), Ok(resolved)) => {
                        match status_instance(&transport, &resolved).await {
                            Ok(status) => json!({
                                "host_name": host.host_name,
                                "reachable": true,
                                "active": status.active,
                                "state": status.systemd_state,
                                "version": status.version_display(),
                            }),
                            Err(e) => unreachable_json(&host.host_name, &format!("{e:#}")),
                        }
                    }
                    (Err(e), _) | (_, Err(e)) => unreachable_json(&host.host_name, &e),
                }
            }
        });
        let hosts = futures_util::future::join_all(checks).await;
        Ok(json!({ "node_id": node_id, "hosts": hosts }))
    }

    async fn connection_info(&self, node_id: &str, host_name: &str) -> Result<Value, String> {
        let node = self.node(node_id)?;
        let host = node.host(host_name).ok_or("No such server in this node")?;
        let resolved = resolve_host(&node.inventory, host_name)?;
        let configured = unbracket(&resolved.instance.public_host).parse().ok();
        let key = self.store.lock().existing_client_key();
        let transport = host_transport(host, key.as_ref())?;
        let addresses = probe_addresses(&transport, configured).await?;
        let can_upgrade = listen_bind(&resolved) == "0.0.0.0" && addresses.ipv6.is_some();
        let mut result = serde_json::to_value(addresses).map_err(|e| e.to_string())?;
        result["ws_port"] = json!(resolved.ws_port);
        result["relay_port"] = json!(resolved.relay_port);
        result["signaling_ipv6"] = json!(signaling_ipv6(&resolved));
        result["dual_stack_on_apply"] = json!(can_upgrade);
        Ok(result)
    }

    async fn logs(
        &self,
        node_id: &str,
        host_name: Option<&str>,
        lines: Option<u32>,
    ) -> Result<Value, String> {
        let node = self.node(node_id)?;
        let host = match host_name {
            Some(name) => node.host(name).ok_or("No such server in this node")?,
            None => node.hosts.first().ok_or("This node has no servers")?,
        };
        let key = self.store.lock().existing_client_key();
        let transport = host_transport(host, key.as_ref())?;
        let resolved = resolve_host(&node.inventory, &host.host_name)?;
        let text = logs_instance_text(&transport, &resolved, false, lines.unwrap_or(200).min(2000))
            .await
            .map_err(|e| format!("{e:#}"))?;
        Ok(json!({ "host_name": host.host_name, "text": text }))
    }

    /// The link a person accepts to add this supernode. Read from the server,
    /// not from the copy saved at setup: a public-address change remints it.
    /// The first server that answers supplies it, and that answer replaces
    /// the saved copy.
    async fn invite(&self, node_id: &str) -> Result<Value, String> {
        let node = self.node(node_id)?;
        if node.hosts.is_empty() {
            return Err("This node has no servers".into());
        }
        let key = self.store.lock().existing_client_key();
        let mut last_error = String::new();
        for host in &node.hosts {
            let transport = match host_transport(host, key.as_ref()) {
                Ok(transport) => transport,
                Err(e) => {
                    last_error = format!("{}: {e}", host.host_name);
                    continue;
                }
            };
            let resolved = match resolve_host(&node.inventory, &host.host_name) {
                Ok(resolved) => resolved,
                Err(e) => {
                    last_error = format!("{}: {e}", host.host_name);
                    continue;
                }
            };
            match invite_instance(&transport, &resolved).await {
                Ok(info) => {
                    let url = info.invite_url;
                    // The panel can still hand the link out if remembering it
                    // fails. The server file remains the source.
                    if let Err(e) = self.edit_node(node_id, |n| n.invite_url = url.clone()) {
                        tracing::warn!("[supernode-admin] could not store the invite: {e}");
                    }
                    return Ok(json!({
                        "invite_url": url,
                        "host_name": host.host_name,
                    }));
                }
                Err(e) => last_error = format!("{}: {e:#}", host.host_name),
            }
        }
        Err(if last_error.is_empty() {
            "Could not read the invite".into()
        } else {
            last_error
        })
    }

    /// Run `action` on each server of a node in turn, collecting its lines
    /// prefixed with the server's name. Stops at the first failure. Going one
    /// server at a time keeps a cluster reachable through the others while
    /// each restarts.
    async fn for_each_host(
        &self,
        node_id: &str,
        host_name: Option<&str>,
        action: HostAction,
    ) -> Result<Vec<String>, String> {
        let node = self.node(node_id)?;
        let key = self.store.lock().existing_client_key();
        let mut out = Vec::new();
        for host in selected_hosts(&node, host_name)? {
            let transport = host_transport(host, key.as_ref())?;
            let resolved = resolve_host(&node.inventory, &host.host_name)?;
            let lines = run_host_action(action, &transport, &resolved)
                .await
                .map_err(|e| format!("{}: {e}", host.host_name))?;
            out.extend(
                lines
                    .into_iter()
                    .map(|l| format!("{}: {l}", host.host_name)),
            );
        }
        Ok(out)
    }

    /// Compare each server's installed package with the published one. The
    /// client reads the release itself, so this works for servers that cannot
    /// reach GitHub.
    async fn check_update(&self, node_id: &str, host_name: Option<&str>) -> Result<Value, String> {
        let node = self.node(node_id)?;
        let selected = selected_hosts(&node, host_name)?;
        let key = self.store.lock().existing_client_key();
        let mut published: HashMap<String, String> = HashMap::new();
        let mut hosts = Vec::new();
        for host in selected {
            if !published.contains_key(&host.platform) {
                let digest = published_release_sha256(&node.inventory.defaults, &host.platform)
                    .await
                    .map_err(|e| format!("read the published release: {e:#}"))?;
                published.insert(host.platform.clone(), digest);
            }
            let transport = host_transport(host, key.as_ref())?;
            let installed = installed_package(&transport, node.inventory.defaults.privilege)
                .await
                .map_err(|e| format!("{}: {e:#}", host.host_name))?;
            let available =
                installed.as_deref() != published.get(&host.platform).map(String::as_str);
            hosts.push(json!({ "host_name": host.host_name, "update_available": available }));
        }
        let any = hosts.iter().any(|h| h["update_available"] == true);
        Ok(json!({ "update_available": any, "hosts": hosts }))
    }

    /// Download and verify the release here, then push it to each server in
    /// turn, which installs it and rolls back if the node fails to start. One
    /// server at a time keeps a cluster reachable through the others.
    async fn update_now(&self, node_id: &str, host_name: Option<&str>) -> Result<Value, String> {
        let node = self.node(node_id)?;
        let selected = selected_hosts(&node, host_name)?;
        let key = self.store.lock().existing_client_key();
        let privilege = node.inventory.defaults.privilege;
        let mut packages: HashMap<String, (Vec<u8>, String)> = HashMap::new();
        let mut lines = Vec::new();
        let mut hosts = Vec::new();
        for host in selected {
            hosts.push(json!({ "host_name": host.host_name, "update_available": false }));
            if !packages.contains_key(&host.platform) {
                let package = download_release_archive(&node.inventory.defaults, &host.platform)
                    .await
                    .map_err(|e| format!("download the release: {e:#}"))?;
                packages.insert(host.platform.clone(), package);
            }
            let (archive, digest) = &packages[&host.platform];
            let transport = host_transport(host, key.as_ref())?;
            let installed = installed_package(&transport, privilege)
                .await
                .map_err(|e| format!("{}: {e:#}", host.host_name))?;
            if installed.as_deref() == Some(digest.as_str()) {
                lines.push(format!(
                    "{}: already running the latest build",
                    host.host_name
                ));
                continue;
            }
            let outcome = push_update(&transport, privilege, archive, digest)
                .await
                .map_err(|e| format!("{}: {e:#}", host.host_name))?;
            if !outcome.succeeded {
                return Err(format!(
                    "{}: update failed: {}",
                    host.host_name,
                    outcome.log.join("\n")
                ));
            }
            lines.extend(
                outcome
                    .log
                    .iter()
                    .map(|l| format!("{}: {l}", host.host_name)),
            );
        }
        Ok(json!({ "lines": lines, "hosts": hosts }))
    }

    fn node(&self, node_id: &str) -> Result<ManagedNode, String> {
        self.store
            .lock()
            .node(node_id)
            .cloned()
            .ok_or_else(|| "That supernode is no longer managed here".to_string())
    }

    async fn set_hostname(
        &self,
        node_id: &str,
        host_name: &str,
        settings: HostnameSettings,
        update: UpdateRequest,
    ) -> Result<Value, String> {
        let settings = settings.checked()?;
        let plan = update.plan(&settings.hostname, &settings.hostname_lookup)?;
        let _dddns_turn = self.dddns_turn(plan.as_ref()).await;
        let mut node = self.node(node_id)?;
        let host = node.host(host_name).ok_or("No such server in this node")?;
        let key = self.store.lock().existing_client_key();
        let transport = host_transport(host, key.as_ref())?;
        let roster = match &node.cluster_id {
            Some(id) => Some(
                ClusterCache::load(&self.cluster_cache)
                    .map_err(|e| format!("{e:#}"))?
                    .find_roster(id)
                    .ok_or("Sync the cluster before changing a server's hostname")?,
            ),
            None => None,
        };
        let bind = {
            let resolved = resolve_host(&node.inventory, host_name)?;
            if matches!(listen_bind(&resolved), "0.0.0.0" | "::") {
                Some(probe_listen_bind(&transport).await?)
            } else {
                None
            }
        };
        set_host_hostname(&mut node, host_name, &settings)?;
        if let Some(bind) = &bind {
            set_host_bind(&mut node, host_name, bind)?;
        }
        let resolved = resolve_host(&node.inventory, host_name)?;
        let mut lines =
            push_config_instance_report(&transport, &resolved, true, roster.as_ref(), false)
                .await
                .map_err(|e| {
                    format!("Could not apply the hostname; the server may need a retry: {e:#}")
                })?;
        self.edit_node(node_id, |node| {
            set_host_hostname(node, host_name, &settings)?;
            if let Some(bind) = &bind {
                set_host_bind(node, host_name, bind)?;
            }
            Ok::<(), String>(())
        })??;
        if let Some(plan) = plan {
            self.publish_hostname(node_id, host_name, &plan, &update).await
                .map_err(|error| format!("Hostname saved and applied, but the DDDNS update could not be confirmed: {error}. Re-enter the token and apply again."))?;
            lines.push(format!(
                "DDDNS updated: {} now points to this server.",
                settings.hostname
            ));
        }
        Ok(json!({ "lines": lines, "state": self.state() }))
    }

    async fn publish_hostname(
        &self,
        node_id: &str,
        host_name: &str,
        plan: &UpdatePlan,
        update: &UpdateRequest,
    ) -> Result<(), String> {
        let node = self.node(node_id)?;
        let host = node.host(host_name).ok_or("No such server in this node")?;
        let resolved = resolve_host(&node.inventory, host_name)?;
        let key = self.store.lock().existing_client_key();
        let transport = host_transport(host, key.as_ref())?;
        let configured = unbracket(&resolved.instance.public_host).parse().ok();
        let addresses = probe_addresses(&transport, configured).await?;
        plan.publish(
            update,
            addresses,
            resolved.ws_port,
            signaling_ipv6(&resolved),
        )
        .await
    }

    async fn dddns_turn(
        &self,
        plan: Option<&UpdatePlan>,
    ) -> Option<tokio::sync::OwnedMutexGuard<()>> {
        let plan = plan?;
        let turn = {
            let mut changes = self.dddns_changes.lock();
            changes.retain(|_, turn| turn.strong_count() > 0);
            let key = plan.key();
            if let Some(turn) = changes.get(&key).and_then(Weak::upgrade) {
                turn
            } else {
                let turn = Arc::new(tokio::sync::Mutex::new(()));
                changes.insert(key, Arc::downgrade(&turn));
                turn
            }
        };
        Some(turn.lock_owned().await)
    }

    async fn publish_after_setup(
        &self,
        node_id: &str,
        host_name: &str,
        plan: Option<&UpdatePlan>,
        update: &UpdateRequest,
    ) -> Option<String> {
        let plan = plan?;
        self.publish_hostname(node_id, host_name, plan, update).await.err().map(|error|
            format!("Server setup completed, but the DDDNS update could not be confirmed: {error}. Open this server in Your servers and apply the hostname with its update token to retry."))
    }

    fn edit_node<T>(
        &self,
        node_id: &str,
        edit: impl FnOnce(&mut ManagedNode) -> T,
    ) -> Result<T, String> {
        self.store
            .lock()
            .edit(node_id, edit)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "That supernode is no longer managed here".to_string())
    }
}

fn set_host_hostname(
    node: &mut ManagedNode,
    host_name: &str,
    settings: &HostnameSettings,
) -> Result<(), String> {
    let host = node
        .inventory
        .host
        .iter_mut()
        .find(|h| h.name == host_name)
        .ok_or("No inventory for this server")?;
    if host.instances.len() != 1 {
        return Err("Expected one supernode instance on this managed server".into());
    }
    settings.apply(&mut host.instances[0]);
    Ok(())
}

fn set_host_bind(node: &mut ManagedNode, host_name: &str, bind: &str) -> Result<(), String> {
    let host = node
        .inventory
        .host
        .iter_mut()
        .find(|h| h.name == host_name)
        .ok_or("No inventory for this server")?;
    if host.instances.len() != 1 {
        return Err("Expected one supernode instance on this managed server".into());
    }
    host.instances[0].listen_bind = Some(bind.to_owned());
    Ok(())
}

/// The node's cluster id: the one it was linked under, else the one a failed
/// link recorded, else a new one from the node id.
fn cluster_id_for(node: &ManagedNode) -> String {
    node.cluster_id
        .clone()
        .or_else(|| node.inventory.clusters.first().map(|c| c.id.clone()))
        .unwrap_or_else(|| format!("ds-{}", node.id.chars().take(8).collect::<String>()))
}

/// Every supernode instance in the inventory, as one cluster.
fn cluster_def(cluster_id: &str, inventory: &Inventory) -> ClusterDef {
    ClusterDef {
        id: cluster_id.to_owned(),
        members: inventory
            .host
            .iter()
            .flat_map(|h| {
                h.instances
                    .iter()
                    .map(move |i| format!("{}/{}", h.name, i.id))
            })
            .collect(),
        cluster_port: None,
    }
}

fn selected_hosts<'a>(
    node: &'a ManagedNode,
    host_name: Option<&str>,
) -> Result<Vec<&'a ManagedHost>, String> {
    match host_name {
        Some(name) => node
            .host(name)
            .map(|host| vec![host])
            .ok_or_else(|| format!("No such server in this node: {name}")),
        None if node.hosts.is_empty() => Err("This node has no servers".into()),
        None => Ok(node.hosts.iter().collect()),
    }
}

#[derive(Clone, Copy)]
enum HostAction {
    Restart,
    SetUpdateMode(UpdateMode),
    Uninstall { purge: bool },
}

async fn run_host_action(
    action: HostAction,
    transport: &SshTransport,
    resolved: &ResolvedInstance<'_>,
) -> Result<Vec<String>, String> {
    let privilege = resolved.defaults.privilege;
    let detail = |e: anyhow::Error| format!("{e:#}");
    match action {
        HostAction::Restart => lifecycle_report(transport, resolved, LifecycleAction::Restart)
            .await
            .map_err(detail),
        HostAction::SetUpdateMode(mode) => {
            if mode == UpdateMode::ServerTimer
                && !probe_host_os(transport)
                    .await
                    .map_err(detail)?
                    .reaches_github
            {
                return Err(
                    "this server cannot reach github.com to fetch its own updates; \
                            keep updating from DoubleSlash"
                        .into(),
                );
            }
            set_update_mode(transport, privilege, mode)
                .await
                .map_err(detail)
        }
        HostAction::Uninstall { purge } => {
            // Stop the timer first so it does not reinstall a binary for a
            // node that is gone. A host set up before the updater has none.
            let mut lines = set_update_mode(transport, privilege, UpdateMode::ClientDriven)
                .await
                .unwrap_or_default();
            lines.extend(
                uninstall_instance_report(transport, resolved, purge)
                    .await
                    .map_err(detail)?,
            );
            Ok(lines)
        }
    }
}

fn guided_setup(
    server: &ServerRequest,
    checked: &CheckedServer,
    host_name: String,
    client_key: Option<PrivateKey>,
    update_mode: UpdateMode,
) -> GuidedSetup {
    let mut instance = GuidedSetup::default_instance(&checked.public_host);
    checked.hostname.apply(&mut instance);
    instance.allow_public_rooms = Some(server.allow_public_rooms);
    // Deploy/add checks the target's IPv6 availability over pinned SSH before
    // installation, independently of the family of its advertised public IP.
    GuidedSetup {
        host_name,
        ssh_target: checked.ssh_target.clone(),
        public_host: checked.public_host.clone(),
        host_key: checked.host_key.clone(),
        access: checked.access,
        client_key,
        password: server.password.clone().filter(|p| !p.is_empty()),
        hardening: server.hardening(),
        update_mode,
        channel: CHANNEL.into(),
        instance,
    }
}

/// The credentials a setup starts with, for a probe before it runs.
fn setup_auth(setup: &GuidedSetup) -> SessionAuth {
    let mut auth = SessionAuth::pinned(setup.host_key.clone());
    if let Some(key) = &setup.client_key {
        auth = auth.with_key(key.clone());
    }
    if setup.access == SshAccessMode::OwnKey {
        auth = auth.with_default_identity_files();
    }
    if let Some(password) = &setup.password {
        auth = auth.with_password(password.clone());
    }
    auth
}

/// Emit the checklist, then run the setup, emitting each step's state.
async fn run_with_progress(
    setup: &GuidedSetup,
    extra_steps: &[&str],
    emit: Emit<'_>,
) -> Result<snm_supernode::GuidedOutcome, String> {
    let mut titles: Vec<String> = guided_steps(setup).iter().map(|s| s.title()).collect();
    titles.extend(extra_steps.iter().map(|s| s.to_string()));
    emit(json!({ "kind": "plan", "steps": titles }));
    let progress = |p: GuidedProgress| emit(progress_json(&p));
    run_guided_setup(setup, &progress)
        .await
        .map_err(|e| format!("{e:#}"))
}

fn progress_json(progress: &GuidedProgress) -> Value {
    let (state, lines, error) = match &progress.state {
        StepState::Running => ("running", Vec::new(), None),
        StepState::Done(lines) => ("done", lines.clone(), None),
        StepState::Failed(error) => ("failed", Vec::new(), Some(error.clone())),
    };
    json!({
        "kind": "step",
        "index": progress.index,
        "title": progress.step.title(),
        "state": state,
        "lines": lines,
        "error": error,
    })
}

fn managed_host(setup: &GuidedSetup, outcome: &snm_supernode::GuidedOutcome) -> ManagedHost {
    ManagedHost {
        host_name: setup.host_name.clone(),
        ssh_target: setup.ssh_target.clone(),
        host_key: setup.host_key.to_openssh().unwrap_or_default(),
        access: setup.access.as_str().into(),
        public_host: setup.public_host.clone(),
        platform: outcome.platform.clone(),
        node_identity: outcome.node_identity.clone(),
        os: outcome.host_os.display_name(),
    }
}

/// What a provider-side firewall must also allow; the server's own firewall
/// is already set.
fn ports_note(setup: &GuidedSetup) -> Value {
    json!({
        "relay_udp": setup.instance.relay_port,
        "signaling_tcp": setup.instance.ws_port,
        "cluster_udp": setup.instance.cluster_port,
    })
}

/// Credentials for later management: the profile key, or the user's own.
/// No password is kept, so a host set up with `own-key` needs that key.
fn host_transport(host: &ManagedHost, key: Option<&PrivateKey>) -> Result<SshTransport, String> {
    let host_key = parse_public_key(&host.host_key)
        .map_err(|e| format!("{}: stored host key: {e}", host.host_name))?;
    let mut auth = SessionAuth::pinned(host_key);
    match SshAccessMode::parse(&host.access) {
        Some(SshAccessMode::OwnKey) => auth = auth.with_default_identity_files(),
        Some(_) => {
            let key = key.ok_or("This profile's SSH key is missing")?;
            auth = auth.with_key(key.clone());
        }
        None => return Err(format!("{}: unknown access mode", host.host_name)),
    }
    Ok(SshTransport::with_auth(&host.ssh_target, auth))
}

fn resolve_host<'a>(
    inventory: &'a Inventory,
    host_name: &str,
) -> Result<ResolvedInstance<'a>, String> {
    inventory
        .resolve_instances(&Selector::from_flags(Some(host_name.into()), None, false))
        .map_err(|e| e.to_string())?
        .into_iter()
        .next()
        .ok_or_else(|| format!("{host_name} has no supernode instance"))
}

fn node_json(node: &ManagedNode) -> Value {
    let hosts: Vec<Value> = node
        .hosts
        .iter()
        .map(|h| {
            let instance = resolve_host(&node.inventory, &h.host_name).ok();
            let public_host = instance.as_ref().map_or(h.public_host.as_str(), |r| r.instance.public_host.as_str());
            let public_ip = unbracket(public_host).parse::<IpAddr>().ok().map(|ip| ip.to_string());
            let addresses = PublicAddresses::configured(unbracket(public_host).parse().ok());
            json!({
                "host_name": h.host_name,
                "ssh_target": h.ssh_target,
                "public_host": h.public_host,
                "public_ip": public_ip,
                "ipv4": addresses.ipv4,
                "ipv6": addresses.ipv6,
                "ws_port": instance.as_ref().map(|r| r.ws_port),
                "relay_port": instance.as_ref().map(|r| r.relay_port),
                "signaling_ipv6": instance.as_ref().and_then(signaling_ipv6),
                "hostname": instance.as_ref().and_then(|r| r.instance.hostname.as_deref()).unwrap_or(""),
                "hostname_lookup": instance.as_ref().and_then(|r| r.instance.hostname_lookup.as_deref()).unwrap_or(""),
                "access": h.access,
                "platform": h.platform,
                "os": h.os,
                "node_identity": h.node_identity,
                "host_key_fingerprint": parse_public_key(&h.host_key)
                    .map(|k| host_key_fingerprint(&k))
                    .unwrap_or_default(),
            })
        })
        .collect();
    json!({
        "id": node.id,
        "name": node.name,
        "channel": node.channel,
        "update_mode": node.update_mode,
        "cluster_id": node.cluster_id,
        "invite_url": node.invite_url,
        "created_at": node.created_at,
        "hosts": hosts,
    })
}

fn listen_bind<'a>(resolved: &'a ResolvedInstance<'_>) -> &'a str {
    resolved
        .instance
        .listen_bind
        .as_deref()
        .unwrap_or(&resolved.defaults.supernode.listen_bind)
}

fn signaling_ipv6(resolved: &ResolvedInstance<'_>) -> Option<bool> {
    unbracket(listen_bind(resolved))
        .parse::<IpAddr>()
        .ok()
        .map(|ip| ip.is_ipv6())
}

fn unreachable_json(host_name: &str, error: &str) -> Value {
    json!({ "host_name": host_name, "reachable": false, "error": error })
}

/// An inventory host name from a display name, unique among `taken`.
fn host_slug(name: &str, taken: &[&str]) -> String {
    let mut base: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    base.truncate(40);
    if base.is_empty() {
        base = "server".into();
    }
    let mut candidate = base.clone();
    let mut n = 2;
    while taken.contains(&candidate.as_str()) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use snm_transport::generate_ed25519_key;

    fn service() -> AdminService {
        let dir = std::env::temp_dir().join(format!(
            "ds-admin-service-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = ManagedNodeStore::open_with_key(&[3u8; 32], &dir.join("managed.dat")).unwrap();
        AdminService::new(store, dir)
    }

    fn server_json(extra: &str) -> String {
        let key = generate_ed25519_key("host").unwrap().public_openssh;
        format!(
            r#"{{"op":"deploy","host":"198.51.100.7","host_key":"{key}","access":"client-key-only","update_mode":"server-timer"{extra}}}"#
        )
    }

    fn ignore(_: Value) {}

    fn clustered_node() -> ManagedNode {
        ManagedNode {
            id: "node".into(),
            name: "Test supernode".into(),
            channel: CHANNEL.into(),
            update_mode: "client".into(),
            cluster_id: Some("cluster".into()),
            invite_url: String::new(),
            created_at: 1,
            hosts: ["first", "second", "third"]
                .into_iter()
                .map(|name| ManagedHost {
                    host_name: name.into(),
                    ssh_target: "root@192.0.2.1".into(),
                    host_key: String::new(),
                    access: "own-key".into(),
                    public_host: "192.0.2.1".into(),
                    platform: "linux-x86_64".into(),
                    node_identity: name.into(),
                    os: "Debian".into(),
                })
                .collect(),
            inventory: Inventory {
                defaults: snm_core::Defaults::default(),
                host: Vec::new(),
                clusters: Vec::new(),
            },
        }
    }

    #[test]
    fn member_selection_is_exact_and_cluster_selection_keeps_order() {
        let mut node = clustered_node();
        let selected = selected_hosts(&node, Some("second")).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].host_name, "second");
        let selected: Vec<_> = selected_hosts(&node, None)
            .unwrap()
            .iter()
            .map(|host| host.host_name.as_str())
            .collect();
        assert_eq!(selected, ["first", "second", "third"]);
        assert!(selected_hosts(&node, Some("SECOND")).is_err());
        assert!(selected_hosts(&node, Some("")).is_err());
        node.hosts.clear();
        assert!(selected_hosts(&node, None).is_err());
    }

    #[test]
    fn server_state_exposes_resolved_ports_and_unbracketed_public_ip() {
        let mut node = clustered_node();
        for name in ["first", "second"] {
            let mut instance = GuidedSetup::default_instance("198.51.100.7");
            if name == "first" {
                instance.ws_port = None;
                instance.relay_port = None;
            } else {
                instance.public_host = "[2001:db8::7]".into();
                instance.listen_bind = Some("::".into());
                instance.ws_port = Some(41000);
                instance.relay_port = Some(42000);
            }
            node.inventory.host.push(snm_core::Host {
                name: name.into(),
                ssh: "root@192.0.2.1".into(),
                arch: None,
                instances: vec![instance],
            });
        }
        let state = node_json(&node);
        assert_eq!(state["hosts"][0]["public_ip"], "198.51.100.7");
        assert_eq!(state["hosts"][0]["ipv4"], "198.51.100.7");
        assert!(state["hosts"][0]["ipv6"].is_null());
        assert_eq!(state["hosts"][0]["ws_port"], 34935);
        assert_eq!(state["hosts"][0]["relay_port"], 3478);
        assert_eq!(state["hosts"][0]["signaling_ipv6"], false);
        assert_eq!(state["hosts"][1]["public_ip"], "2001:db8::7");
        assert_eq!(state["hosts"][1]["ipv6"], "2001:db8::7");
        assert_eq!(state["hosts"][1]["ws_port"], 41000);
        assert_eq!(state["hosts"][1]["relay_port"], 42000);
        assert_eq!(state["hosts"][1]["signaling_ipv6"], true);
        // Missing inventory must not invent the usual port for this server.
        assert!(state["hosts"][2]["ws_port"].is_null());
        assert!(state["hosts"][2]["relay_port"].is_null());
    }

    #[tokio::test]
    async fn moves_of_one_dddns_record_wait_while_other_names_stay_independent(
    ) -> anyhow::Result<()> {
        let service = service();
        let update: UpdateRequest = serde_json::from_value(json!({"dddns_token":"ddt_test"}))?;
        let first_plan = update
            .plan("first.doubleslash.space", "")
            .map_err(anyhow::Error::msg)?;
        let other_plan = update
            .plan("other.doubleslash.space", "")
            .map_err(anyhow::Error::msg)?;
        let first = service.dddns_turn(first_plan.as_ref()).await;
        anyhow::ensure!(first.is_some());
        let waiting = service.dddns_turn(first_plan.as_ref());
        tokio::pin!(waiting);
        anyhow::ensure!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut waiting)
                .await
                .is_err()
        );
        let independent = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            service.dddns_turn(other_plan.as_ref()),
        )
        .await?;
        anyhow::ensure!(independent.is_some());
        drop(first);
        anyhow::ensure!(waiting.await.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn unknown_members_are_rejected_before_ssh_or_release_downloads() {
        let service = service();
        service.store.lock().upsert(clustered_node()).unwrap();
        for op in [
            "status",
            "restart",
            "check_update",
            "update_now",
            "logs",
            "set_hostname",
            "connection_info",
        ] {
            let request = serde_json::from_value(json!({
                "op": op, "node_id": "node", "host_name": "not-a-member",
            }))
            .unwrap();
            let error = service.handle(request, &ignore).await.unwrap_err();
            assert!(error.contains("No such server"), "{op}: {error}");
        }
    }

    #[test]
    fn cluster_commands_remain_compatible_and_member_mutations_stay_serialized() {
        for op in ["status", "restart", "check_update", "update_now"] {
            for name in [None, Some("second")] {
                let mut value = json!({ "op": op, "node_id": "node" });
                if let Some(name) = name {
                    value["host_name"] = json!(name);
                }
                let request: Request = serde_json::from_value(value).unwrap();
                assert_eq!(
                    request.claim().is_some(),
                    matches!(op, "restart" | "update_now")
                );
                let host_name = match request {
                    Request::Status { host_name, .. }
                    | Request::Restart { host_name, .. }
                    | Request::CheckUpdate { host_name, .. }
                    | Request::UpdateNow { host_name, .. } => host_name,
                    _ => unreachable!(),
                };
                assert_eq!(host_name.as_deref(), name);
            }
        }
    }

    #[tokio::test]
    async fn deploy_is_refused_until_the_notice_is_accepted() {
        let service = service();
        let request: Request = serde_json::from_str(&server_json("")).unwrap();
        let error = service.handle(request, &ignore).await.unwrap_err();
        assert!(error.contains("responsibility notice"), "{error}");
        let state = service
            .handle(Request::AcceptNotice, &ignore)
            .await
            .unwrap();
        assert_eq!(state["notice_accepted"], true);
        assert!(state["notice"]
            .as_str()
            .unwrap()
            .contains("solely responsible"));
    }

    #[test]
    fn server_requests_are_checked_before_any_connection() {
        let parse = |extra: &str| -> ServerRequest {
            match serde_json::from_str::<Request>(&server_json(extra)).unwrap() {
                Request::Deploy(server) => server,
                _ => unreachable!(),
            }
        };
        let ok = parse(r#","user":"admin","port":2222"#).check().unwrap();
        assert_eq!(ok.ssh_target, "admin@198.51.100.7:2222");
        assert_eq!(ok.public_host, "198.51.100.7");

        let mut v6 = parse("");
        v6.host = "2001:db8::1".into();
        let checked = v6.check().unwrap();
        assert_eq!(checked.ssh_target, "root@[2001:db8::1]:22");
        assert_eq!(checked.public_host, "[2001:db8::1]");
        let mut named = parse("");
        named.host = "node.example.com".into();
        assert!(
            named.check().is_err(),
            "a name alone gives no IP to advertise"
        );
        named.public_host = "198.51.100.8".into();
        assert_eq!(named.check().unwrap().public_host, "198.51.100.8");
        let mut bad_host = parse("");
        bad_host.host = "host;rm".into();
        assert!(bad_host.check().is_err());
        let mut injected = parse("");
        injected.user = "root; reboot".into();
        assert!(injected.check().is_err());
        let mut no_key = parse("");
        no_key.host_key = "not a key".into();
        assert!(no_key.check().is_err());
        let mut unknown_access = parse("");
        unknown_access.access = "telnet".into();
        assert!(unknown_access.check().is_err());
    }

    #[test]
    fn hostname_setup_and_edits_preserve_other_server_settings() {
        let request: Request = serde_json::from_str(&server_json(
            r#", "hostname":" Mine.Names.Example. ", "hostname_lookup":"DDDNS", "dddns_token":"ddt_test", "dddns_update_url":"https://users.names.example/v1/update""#,
        ))
        .unwrap();
        let Request::Deploy(server) = request else {
            unreachable!()
        };
        let checked = server.check().unwrap();
        let setup = guided_setup(
            &server,
            &checked,
            "first".into(),
            None,
            UpdateMode::ClientDriven,
        );
        assert_eq!(
            setup.instance.hostname.as_deref(),
            Some("mine.names.example")
        );
        assert_eq!(setup.instance.hostname_lookup.as_deref(), Some("dddns"));
        assert_eq!(setup.instance.public_host, "198.51.100.7");

        let mut node = clustered_node();
        for host in &node.hosts {
            node.inventory.host.push(snm_core::Host {
                name: host.host_name.clone(),
                ssh: host.ssh_target.clone(),
                arch: None,
                instances: vec![setup.instance.clone()],
            });
        }
        let service = service();
        service.store.lock().upsert(node.clone()).unwrap();
        let settings = HostnameSettings {
            hostname: "relay.example.com".into(),
            hostname_lookup: "dns".into(),
        }
        .checked()
        .unwrap();
        service
            .edit_node("node", |node| {
                set_host_hostname(node, "second", &settings)?;
                set_host_bind(node, "second", "::")
            })
            .unwrap()
            .unwrap();
        let reopened = ManagedNodeStore::open_with_key(
            &[3u8; 32],
            &service.cluster_cache.with_file_name("managed.dat"),
        )
        .unwrap();
        let stored = reopened.node("node").unwrap().clone();
        assert_eq!(
            resolve_host(&stored.inventory, "first")
                .unwrap()
                .instance
                .hostname
                .as_deref(),
            Some("mine.names.example")
        );
        let second = resolve_host(&stored.inventory, "second").unwrap();
        assert_eq!(
            second.instance.hostname.as_deref(),
            Some("relay.example.com")
        );
        assert_eq!(second.instance.public_host, "198.51.100.7");
        assert_eq!(signaling_ipv6(&second), Some(true));
        assert_eq!(
            listen_bind(&resolve_host(&stored.inventory, "first").unwrap()),
            "0.0.0.0"
        );
        assert_eq!(stored.hosts[1].ssh_target, node.hosts[1].ssh_target);
        assert_eq!(stored.hosts[1].node_identity, node.hosts[1].node_identity);
        assert_eq!(stored.cluster_id, node.cluster_id);
        assert_eq!(node_json(&stored)["hosts"][1]["hostname_lookup"], "dns");
        assert!(!serde_json::to_string(&stored).unwrap().contains("ddt_test"));
        assert!(!node_json(&stored).to_string().contains("ddt_test"));
        let mut cleared = stored;
        set_host_hostname(&mut cleared, "second", &HostnameSettings::default()).unwrap();
        assert!(resolve_host(&cleared.inventory, "second")
            .unwrap()
            .instance
            .hostname
            .is_none());
        assert!(resolve_host(&cleared.inventory, "second")
            .unwrap()
            .instance
            .hostname_lookup
            .is_none());
        assert!(set_host_hostname(&mut cleared, "missing", &settings).is_err());
    }

    #[tokio::test]
    async fn invalid_hostname_edits_fail_before_ssh_and_take_the_node_lock() {
        let service = service();
        service.store.lock().upsert(clustered_node()).unwrap();
        for (hostname, lookup) in [
            ("ws://relay.example.com", "dns"),
            ("relay.example.com:34935", "dns"),
            ("198.51.100.7", "dns"),
            ("bad_host.example", "dns"),
            ("relay.example.com", "unknown"),
            ("localhost", "dddns"),
        ] {
            let request: Request = serde_json::from_value(json!({
                "op": "set_hostname", "node_id": "node", "host_name": "second",
                "hostname": hostname, "hostname_lookup": lookup,
            }))
            .unwrap();
            assert_eq!(
                request.claim(),
                Some(Claim {
                    setup: false,
                    node: Some("node")
                })
            );
            assert!(service.handle(request, &ignore).await.is_err());
        }
        assert!(!service.has_tasks());
        let automatic = HostnameSettings {
            hostname: "Node.DoubleSlash.Space.".into(),
            hostname_lookup: String::new(),
        }
        .checked()
        .unwrap();
        assert_eq!(automatic.hostname, "node.doubleslash.space");
        assert!(automatic.hostname_lookup.is_empty());
    }

    #[tokio::test]
    async fn hostname_edit_requires_the_cluster_roster_before_contacting_a_server() {
        let service = service();
        let mut node = clustered_node();
        node.hosts[1].host_key = generate_ed25519_key("server").unwrap().public_openssh;
        service.store.lock().upsert(node).unwrap();
        let request = serde_json::from_value(json!({
            "op": "set_hostname", "node_id": "node", "host_name": "second",
            "hostname": "relay.example.com", "hostname_lookup": "dns",
        }))
        .unwrap();
        let error = service.handle(request, &ignore).await.unwrap_err();
        assert!(error.contains("Sync the cluster"), "{error}");
        assert!(!service.has_tasks());
        assert!(service.node("node").unwrap().inventory.host.is_empty());
    }

    #[test]
    fn reboot_needs_automatic_updates() {
        let request: Request = serde_json::from_str(&server_json(
            r#","hardening":{"automatic_security_updates":false,"automatic_reboot":true}"#,
        ))
        .unwrap();
        let Request::Deploy(server) = request else {
            unreachable!()
        };
        let hardening = server.hardening();
        assert!(!hardening.automatic_reboot);
        assert!(hardening.firewall && hardening.fail2ban);
    }

    #[test]
    fn host_slugs_are_plain_and_unique() {
        assert_eq!(host_slug("My Node!", &[]), "my-node");
        assert_eq!(host_slug("My Node", &["my-node"]), "my-node-2");
        assert_eq!(host_slug("***", &[]), "server");
    }

    #[test]
    fn requests_can_run_on_the_runtime() {
        fn require_send<T: Send>(_: &T) {}
        let service = service();
        let future = service.handle(Request::State, &ignore);
        require_send(&future);
    }

    fn request(value: Value) -> Request {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn tasks_claim_their_supernode_and_setups_claim_the_setup() {
        let node = |id| Claim {
            setup: false,
            node: Some(id),
        };
        for op in [
            "restart",
            "update_now",
            "uninstall",
            "forget",
            "sync_cluster",
        ] {
            assert_eq!(
                request(json!({ "op": op, "node_id": "x" })).claim(),
                Some(node("x")),
                "{op}"
            );
        }
        assert_eq!(
            request(json!({ "op": "set_update_mode", "node_id": "x", "mode": "client" })).claim(),
            Some(node("x"))
        );
        assert_eq!(
            request(json!({
                "op": "remove_server", "node_id": "x", "host_name": "h", "uninstall": true,
            }))
            .claim(),
            Some(node("x"))
        );
        for op in ["status", "invite", "check_update", "logs"] {
            assert_eq!(request(json!({ "op": op, "node_id": "x" })).claim(), None);
        }
        assert_eq!(
            request(json!({ "op": "rename", "node_id": "x", "name": "n" })).claim(),
            None
        );
        let Ok(Request::Deploy(server)) = serde_json::from_str::<Request>(&server_json("")) else {
            unreachable!()
        };
        assert_eq!(
            Request::Deploy(server).claim(),
            Some(Claim {
                setup: true,
                node: None
            })
        );
    }

    #[test]
    fn remove_server_must_say_whether_to_uninstall() {
        let missing = serde_json::from_value::<Request>(json!({
            "op": "remove_server", "node_id": "x", "host_name": "h",
        }));
        assert!(missing.is_err());
    }

    #[test]
    fn different_supernodes_run_side_by_side_but_one_supernode_runs_one_task() {
        let service = service();
        let restart_a = request(json!({ "op": "restart", "node_id": "a" }));
        let update_a = request(json!({ "op": "update_now", "node_id": "a" }));
        let restart_b = request(json!({ "op": "restart", "node_id": "b" }));
        let status_a = request(json!({ "op": "status", "node_id": "a" }));

        let a = service.begin(&restart_a).unwrap().unwrap();
        let b = service.begin(&restart_b).unwrap().unwrap();
        let busy = service.begin(&update_a).err().unwrap();
        assert!(busy.contains("busy"), "{busy}");
        assert!(service.begin(&status_a).unwrap().is_none());
        assert_eq!(service.state()["busy_nodes"], json!(["a", "b"]));
        assert!(service.has_tasks());

        drop(a);
        assert_eq!(service.state()["busy_nodes"], json!(["b"]));
        assert!(service.begin(&update_a).unwrap().is_some());
        drop(b);
        assert!(!service.has_tasks());
        assert_eq!(service.state()["setup_running"], false);
    }

    #[test]
    fn one_setup_runs_at_a_time_beside_other_tasks() {
        let service = service();
        let setup = service.claim(true, None).unwrap();
        assert_eq!(service.state()["setup_running"], true);
        let add = request(json!({
            "op": "add_server", "node_id": "a", "host": "198.51.100.9", "host_key": "k",
            "access": "client-key-only",
        }));
        assert!(service.begin(&add).err().unwrap().contains("setup"));
        assert!(service
            .begin(&request(json!({ "op": "restart", "node_id": "a" })))
            .unwrap()
            .is_some());
        drop(setup);
        assert_eq!(service.state()["setup_running"], false);
    }

    #[tokio::test]
    async fn removing_a_server_needs_another_one_left() {
        let service = service();
        let mut single = clustered_node();
        single.hosts.truncate(1);
        service.store.lock().upsert(single).unwrap();
        let error = service
            .handle(
                request(json!({
                    "op": "remove_server", "node_id": "node", "host_name": "first",
                    "uninstall": false,
                })),
                &ignore,
            )
            .await
            .unwrap_err();
        assert!(error.contains("only server"), "{error}");
        let error = service
            .handle(
                request(json!({
                    "op": "remove_server", "node_id": "node", "host_name": "missing",
                    "uninstall": false,
                })),
                &ignore,
            )
            .await
            .unwrap_err();
        assert!(error.contains("No such server"), "{error}");
        assert_eq!(service.node("node").unwrap().hosts.len(), 1);
        assert!(!service.has_tasks(), "a refused task frees its claim");
    }

    #[tokio::test]
    async fn sync_needs_a_cluster() {
        let service = service();
        let mut single = clustered_node();
        single.hosts.truncate(1);
        single.cluster_id = None;
        service.store.lock().upsert(single).unwrap();
        let error = service
            .handle(
                request(json!({ "op": "sync_cluster", "node_id": "node" })),
                &ignore,
            )
            .await
            .unwrap_err();
        assert!(error.contains("Add a cluster member"), "{error}");
    }

    #[tokio::test]
    async fn rename_is_local_and_cleaned() {
        let service = service();
        service.store.lock().upsert(clustered_node()).unwrap();
        let state = service
            .handle(
                request(json!({
                    "op": "rename", "node_id": "node", "name": "  \u{202e}Edge\n west  ",
                })),
                &ignore,
            )
            .await
            .unwrap();
        assert_eq!(state["nodes"][0]["name"], "Edge west");
        let error = service
            .handle(
                request(json!({ "op": "rename", "node_id": "node", "name": " \u{200e} " })),
                &ignore,
            )
            .await
            .unwrap_err();
        assert!(error.contains("Enter a name"), "{error}");
        let error = service
            .handle(
                request(json!({ "op": "rename", "node_id": "gone", "name": "x" })),
                &ignore,
            )
            .await
            .unwrap_err();
        assert!(error.contains("no longer managed"), "{error}");
    }

    #[test]
    fn cluster_ids_survive_a_failed_link() {
        let mut node = clustered_node();
        assert_eq!(cluster_id_for(&node), "cluster");
        node.cluster_id = None;
        assert_eq!(cluster_id_for(&node), "ds-node");
        node.inventory.clusters = vec![ClusterDef {
            id: "ds-recorded".into(),
            members: Vec::new(),
            cluster_port: None,
        }];
        assert_eq!(cluster_id_for(&node), "ds-recorded");
    }

    #[tokio::test]
    async fn invite_for_an_unknown_node_does_not_connect() {
        let service = service();
        let request: Request =
            serde_json::from_str(r#"{"op":"invite","node_id":"missing"}"#).unwrap();
        let error = service.handle(request, &ignore).await.unwrap_err();
        assert!(error.contains("no longer managed"), "{error}");
    }

    #[test]
    fn stored_hosts_need_the_matching_credentials() {
        let host_key = generate_ed25519_key("host").unwrap().public_openssh;
        let host = ManagedHost {
            host_name: "n".into(),
            ssh_target: "root@198.51.100.7:22".into(),
            host_key,
            access: "client-key-only".into(),
            public_host: "198.51.100.7".into(),
            platform: "linux-x86_64".into(),
            node_identity: "id".into(),
            os: String::new(),
        };
        assert!(host_transport(&host, None).is_err());
        let own = ManagedHost {
            access: "own-key".into(),
            ..host.clone()
        };
        assert!(host_transport(&own, None).is_ok());
    }
}
