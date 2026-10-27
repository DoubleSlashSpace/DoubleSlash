//! JSON commands for the supernode admin UI, run on the client's runtime.
//!
//! Each request names an `op`. Long operations report progress through
//! `emit` as `{"kind":"plan"|"step", …}` events; every request ends with one
//! result value or an error string. Nothing here touches Qt, so the same
//! service can back the Android client later.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use snm_core::{ClusterDef, Inventory, PrivilegeMode, ResolvedInstance, Selector};
use snm_supernode::{
    check_for_update, cluster_sync_report_with, guided_steps, lifecycle_report, logs_instance_text,
    probe_host_os, run_guided_setup, run_update_now, set_update_mode, status_instance,
    uninstall_instance_report, GuidedProgress, GuidedSetup, HardeningOptions, LifecycleAction,
    OsSupport, SshAccessMode, StepState, UpdateMode,
};
use snm_transport::{
    fetch_host_key, host_key_fingerprint, parse_public_key, PrivateKey, PublicKey, SessionAuth,
    SshTransport,
};
use zeroize::Zeroize;

use super::store::{ManagedHost, ManagedNode, ManagedNodeStore};

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
    },
    Logs {
        node_id: String,
        #[serde(default)]
        host_name: Option<String>,
        #[serde(default)]
        lines: Option<u32>,
    },
    Restart {
        node_id: String,
    },
    CheckUpdate {
        node_id: String,
    },
    UpdateNow {
        node_id: String,
    },
    SetUpdateMode {
        node_id: String,
        mode: String,
    },
    Uninstall {
        node_id: String,
        #[serde(default)]
        purge: bool,
    },
    Forget {
        node_id: String,
    },
}

impl Request {
    /// Operations that change a server. The UI runs one at a time.
    pub fn is_mutating(&self) -> bool {
        matches!(
            self,
            Self::Deploy(_)
                | Self::AddServer { .. }
                | Self::Restart { .. }
                | Self::UpdateNow { .. }
                | Self::SetUpdateMode { .. }
                | Self::Uninstall { .. }
        )
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
    host_key: PublicKey,
    access: SshAccessMode,
}

impl ServerRequest {
    fn check(&self) -> Result<CheckedServer, String> {
        let host = self.host.trim();
        let user = self.user.trim();
        if !is_plain_host(host) {
            return Err("Enter the server's IPv4 address or host name".into());
        }
        if user.is_empty()
            || !user
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        {
            return Err("Enter a plain login name, such as root".into());
        }
        let public_host = match self.public_host.trim() {
            "" => host,
            other if is_plain_host(other) => other,
            _ => return Err("The public address must be an IPv4 address or host name".into()),
        };
        let host_key = parse_public_key(&self.host_key)
            .map_err(|_| "Confirm the server's host key before continuing".to_string())?;
        let access = SshAccessMode::parse(&self.access)
            .ok_or_else(|| format!("Unknown SSH access choice {:?}", self.access))?;
        Ok(CheckedServer {
            ssh_target: format!("{user}@{host}:{}", self.port),
            public_host: public_host.to_string(),
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

/// IPv4 or a DNS name. IPv6 literals are refused: `user@host:port` targets
/// cannot carry them unbracketed.
fn is_plain_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !host.starts_with('-')
}

pub struct AdminService {
    store: Arc<Mutex<ManagedNodeStore>>,
    /// Cluster roster cache the manager library keeps beside its inventory.
    cluster_cache: PathBuf,
}

impl AdminService {
    pub fn new(store: ManagedNodeStore, profile_dir: PathBuf) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            cluster_cache: profile_dir.join("managed_supernodes_cluster_cache.toml"),
        }
    }

    /// Everything the admin panel shows without contacting a server.
    pub fn state(&self) -> Value {
        let store = self.store.lock();
        let client_public_key = store
            .existing_client_key()
            .and_then(|k| k.public_key().to_openssh().ok());
        let nodes: Vec<Value> = store.nodes().iter().map(node_json).collect();
        json!({
            "notice": RESPONSIBILITY_NOTICE,
            "notice_accepted": store.notice_accepted(),
            "client_public_key": client_public_key,
            "nodes": nodes,
        })
    }

    pub async fn handle(&self, request: Request, emit: Emit<'_>) -> Result<Value, String> {
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
                if !is_plain_host(host.trim()) {
                    return Err("Enter the server's IPv4 address or host name".into());
                }
                let key = fetch_host_key(host.trim(), port)
                    .await
                    .map_err(|e| format!("Could not reach {host}:{port}: {e}"))?;
                Ok(json!({
                    "fingerprint": host_key_fingerprint(&key),
                    "host_key": key.to_openssh().map_err(|e| e.to_string())?,
                }))
            }
            Request::Deploy(server) => self.deploy(server, emit).await,
            Request::AddServer { node_id, server } => self.add_server(&node_id, server, emit).await,
            Request::Status { node_id } => self.status(&node_id).await,
            Request::Logs {
                node_id,
                host_name,
                lines,
            } => self.logs(&node_id, host_name.as_deref(), lines).await,
            Request::Restart { node_id } => {
                let lines = self.for_each_host(&node_id, HostAction::Restart).await?;
                Ok(json!({ "lines": lines }))
            }
            Request::CheckUpdate { node_id } => {
                let lines = self
                    .for_each_host(&node_id, HostAction::CheckUpdate)
                    .await?;
                Ok(json!({
                    "update_available": lines.iter().any(|l| l.ends_with(UPDATE_AVAILABLE)),
                }))
            }
            Request::UpdateNow { node_id } => {
                let lines = self.for_each_host(&node_id, HostAction::UpdateNow).await?;
                Ok(json!({ "lines": lines }))
            }
            Request::SetUpdateMode { node_id, mode } => {
                let parsed = UpdateMode::parse(&mode)
                    .ok_or_else(|| format!("Unknown update mode {mode:?}"))?;
                let lines = self
                    .for_each_host(&node_id, HostAction::SetUpdateMode(parsed))
                    .await?;
                self.edit_node(&node_id, |node| node.update_mode = mode.clone())?;
                Ok(json!({ "lines": lines, "state": self.state() }))
            }
            Request::Uninstall { node_id, purge } => {
                let lines = self
                    .for_each_host(&node_id, HostAction::Uninstall { purge })
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
        let update_mode = server
            .update_mode
            .as_deref()
            .and_then(UpdateMode::parse)
            .ok_or("Choose how the supernode is updated")?;
        let client_key = self.key_for(checked.access)?;
        let name = match server.name.trim() {
            "" => checked.public_host.clone(),
            name => name.to_string(),
        };
        let setup = guided_setup(
            &server,
            &checked,
            host_slug(&name, &[]),
            client_key,
            update_mode,
        );

        let outcome = run_with_progress(&setup, &[], emit).await?;
        let host = managed_host(&setup, &outcome);
        let node = ManagedNode {
            id: uuid::Uuid::new_v4().to_string(),
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
        Ok(json!({
            "node_id": node_id,
            "node_identity": outcome.node_identity,
            "accept_invite": outcome.invite_url,
            "ports": ports_note(&setup),
            "state": self.state(),
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
        let setup = guided_setup(&server, &checked, host_name, client_key, update_mode);

        // The cluster's shared defaults carry one privilege mode, so a new
        // server must be logged into the same way as the first.
        let probe = SshTransport::with_auth(&checked.ssh_target, setup_auth(&setup));
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

        let mut node = node;
        let new_host = managed_host(&setup, &outcome);
        let mut inventory = node.inventory.clone();
        inventory
            .host
            .extend(outcome.inventory.host.iter().cloned());
        let cluster_id = node
            .cluster_id
            .clone()
            .unwrap_or_else(|| format!("ds-{}", &node.id[..8.min(node.id.len())]));
        let cluster = ClusterDef {
            id: cluster_id.clone(),
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
        };
        inventory.clusters = vec![cluster.clone()];
        let mut hosts = node.hosts.clone();
        hosts.push(new_host);

        // Save the server before linking: a failed link must not lose track
        // of a machine that now runs a node.
        node.hosts = hosts.clone();
        node.inventory = inventory.clone();
        self.store
            .lock()
            .upsert(node.clone())
            .map_err(|e| e.to_string())?;

        let key = self.store.lock().existing_client_key();
        let result = cluster_sync_report_with(&inventory, &cluster, &self.cluster_cache, |host| {
            let managed = hosts
                .iter()
                .find(|h| h.host_name == host.name)
                .ok_or_else(|| anyhow::anyhow!("no credentials for {}", host.name))?;
            host_transport(managed, key.as_ref()).map_err(anyhow::Error::msg)
        })
        .await;
        match result {
            Ok(lines) => {
                step("done", lines, None);
                node.cluster_id = Some(cluster_id);
                self.store.lock().upsert(node).map_err(|e| e.to_string())?;
                Ok(
                    json!({ "node_id": node_id, "ports": ports_note(&setup), "state": self.state() }),
                )
            }
            Err(e) => {
                let message = format!("{e:#}");
                step("failed", Vec::new(), Some(message.clone()));
                Err(format!("{LINK_STEP}: {message}"))
            }
        }
    }

    async fn status(&self, node_id: &str) -> Result<Value, String> {
        let node = self.node(node_id)?;
        let key = self.store.lock().existing_client_key();
        let mut hosts = Vec::new();
        for host in &node.hosts {
            let entry = match (
                host_transport(host, key.as_ref()),
                resolve_host(&node.inventory, &host.host_name),
            ) {
                (Ok(transport), Ok(resolved)) => match status_instance(&transport, &resolved).await
                {
                    Ok(status) => json!({
                        "host_name": host.host_name,
                        "reachable": true,
                        "active": status.active,
                        "state": status.systemd_state,
                        "version": status.version_display(),
                    }),
                    Err(e) => unreachable_json(&host.host_name, &format!("{e:#}")),
                },
                (Err(e), _) | (_, Err(e)) => unreachable_json(&host.host_name, &e),
            };
            hosts.push(entry);
        }
        Ok(json!({ "node_id": node_id, "hosts": hosts }))
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

    /// Run `action` on each server of a node in turn, collecting its lines
    /// prefixed with the server's name. Stops at the first failure. Going one
    /// server at a time keeps a cluster reachable through the others while
    /// each restarts.
    async fn for_each_host(
        &self,
        node_id: &str,
        action: HostAction,
    ) -> Result<Vec<String>, String> {
        let node = self.node(node_id)?;
        let key = self.store.lock().existing_client_key();
        let mut out = Vec::new();
        for host in &node.hosts {
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

    fn node(&self, node_id: &str) -> Result<ManagedNode, String> {
        self.store
            .lock()
            .node(node_id)
            .cloned()
            .ok_or_else(|| "That supernode is no longer managed here".to_string())
    }

    fn edit_node(&self, node_id: &str, edit: impl FnOnce(&mut ManagedNode)) -> Result<(), String> {
        let mut node = self.node(node_id)?;
        edit(&mut node);
        self.store.lock().upsert(node).map_err(|e| e.to_string())
    }
}

const UPDATE_AVAILABLE: &str = "update available";

#[derive(Clone, Copy)]
enum HostAction {
    Restart,
    CheckUpdate,
    UpdateNow,
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
        HostAction::CheckUpdate => {
            let available = check_for_update(transport, privilege)
                .await
                .map_err(detail)?;
            Ok(vec![if available {
                UPDATE_AVAILABLE.into()
            } else {
                "up to date".into()
            }])
        }
        HostAction::UpdateNow => {
            let outcome = run_update_now(transport, privilege).await.map_err(detail)?;
            if outcome.succeeded {
                Ok(outcome.log)
            } else {
                Err(format!("update failed: {}", outcome.log.join("\n")))
            }
        }
        HostAction::SetUpdateMode(mode) => set_update_mode(transport, privilege, mode)
            .await
            .map_err(detail),
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
    instance.allow_public_rooms = Some(server.allow_public_rooms);
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
            json!({
                "host_name": h.host_name,
                "ssh_target": h.ssh_target,
                "public_host": h.public_host,
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

        let mut bad_host = parse("");
        bad_host.host = "2001:db8::1".into();
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

    #[test]
    fn mutating_operations_are_flagged() {
        let restart: Request = serde_json::from_str(r#"{"op":"restart","node_id":"x"}"#).unwrap();
        let status: Request = serde_json::from_str(r#"{"op":"status","node_id":"x"}"#).unwrap();
        assert!(restart.is_mutating());
        assert!(!status.is_mutating());
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
