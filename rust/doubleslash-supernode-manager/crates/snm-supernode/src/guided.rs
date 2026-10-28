//! The guided "host a supernode" flow, as one sequence an application drives.
//!
//! The steps are fixed up front ([`guided_steps`]) so a UI can show the whole
//! checklist before anything runs, then tick each one off from the progress
//! callback. Every step is idempotent, so a failed run is retried from the top
//! rather than resumed mid-way. Password login is turned off last, so a
//! failure anywhere earlier leaves the user able to log in as before.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use snm_core::{Defaults, FirewallMode, Host, Instance, Inventory, Selector};
use snm_transport::{PrivateKey, PublicKey, SessionAuth, SshTransport};

use crate::autoupdate::{install_updater, record_installed_package, UpdateMode, UpdaterConfig};
use crate::harden::{
    apply_hardening_step, hardening_steps, probe_host_os, HardeningOptions, HardeningStep, HostOs,
    OsSupport,
};
use crate::invite::{collect_identity_pub, fetch_invite};
use crate::layout::{instance_label, InstanceLayout};
use crate::ops::{install_instance_report, ping_host, resolve_install_binary};
use crate::ssh_access::{
    install_authorized_key, restrict_ssh_to_keys, verify_key_only_login, SshAccessMode,
};

/// Everything the user chose in the wizard.
pub struct GuidedSetup {
    /// Local name for the server in the inventory.
    pub host_name: String,
    /// `user@host[:port]`.
    pub ssh_target: String,
    /// The address clients reach the node at.
    pub public_host: String,
    /// The host key the user confirmed.
    pub host_key: PublicKey,
    pub access: SshAccessMode,
    /// The profile's own key, for the two client-key access modes.
    pub client_key: Option<PrivateKey>,
    /// Used to log in until the client key is installed, and never stored.
    pub password: Option<String>,
    pub hardening: HardeningOptions,
    pub update_mode: UpdateMode,
    /// `nightly` until a stable release exists.
    pub channel: String,
    /// Instance id, ports and room policy.
    pub instance: Instance,
}

impl GuidedSetup {
    /// One instance `a` on the default ports, with the manager's default
    /// features and room policy.
    pub fn default_instance(public_host: &str) -> Instance {
        Instance {
            id: "a".into(),
            public_host: public_host.into(),
            relay_port: Some(snm_core::default_relay_port(0)),
            ws_port: Some(snm_core::default_ws_port(0)),
            listen_bind: None,
            access_mode: None,
            identity_file: None,
            allow_public_rooms: None,
            allow_private_rooms: None,
            cluster_port: Some(snm_core::default_cluster_port(0)),
            features: Vec::new(),
        }
    }

    fn validate(&self) -> Result<()> {
        if self.access.installs_client_key() && self.client_key.is_none() {
            bail!("this access mode installs the client's key, but none was supplied");
        }
        if self.host_name.trim().is_empty() || self.public_host.trim().is_empty() {
            bail!("the server needs a name and a public address");
        }
        Ok(())
    }

    /// Credentials for the first connection: everything the user gave us.
    fn initial_auth(&self) -> SessionAuth {
        let mut auth = SessionAuth::pinned(self.host_key.clone());
        if let Some(key) = &self.client_key {
            auth = auth.with_key(key.clone());
        }
        if self.access == SshAccessMode::OwnKey {
            auth = auth.with_default_identity_files();
        }
        if let Some(password) = &self.password {
            auth = auth.with_password(password.clone());
        }
        auth
    }

    /// Credentials once the client key is known to work. Only the key-only
    /// mode drops the password: the others leave password login on anyway.
    fn key_auth(&self) -> SessionAuth {
        match (&self.client_key, self.access) {
            (Some(key), SshAccessMode::ClientKeyOnly) => {
                SessionAuth::pinned(self.host_key.clone()).with_key(key.clone())
            }
            _ => self.initial_auth(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuidedStep {
    Connect,
    CheckServer,
    InstallKey,
    Harden(HardeningStep),
    InstallNode,
    SetUpUpdates(UpdateMode),
    RestrictSsh,
    FetchInvite,
}

impl GuidedStep {
    pub fn title(&self) -> String {
        match self {
            Self::Connect => "Connect to the server".into(),
            Self::CheckServer => "Check the operating system".into(),
            Self::InstallKey => "Install this device's SSH key".into(),
            Self::Harden(step) => step.title().into(),
            Self::InstallNode => "Install the supernode".into(),
            Self::SetUpUpdates(UpdateMode::ServerTimer) => {
                "Turn on automatic supernode updates".into()
            }
            Self::SetUpUpdates(UpdateMode::ClientDriven) => {
                "Set up updates from DoubleSlash".into()
            }
            Self::RestrictSsh => "Turn off SSH password login".into(),
            Self::FetchInvite => "Get the node's invite".into(),
        }
    }
}

/// The checklist, in the order it runs.
pub fn guided_steps(setup: &GuidedSetup) -> Vec<GuidedStep> {
    let mut steps = vec![GuidedStep::Connect, GuidedStep::CheckServer];
    if setup.access.installs_client_key() {
        steps.push(GuidedStep::InstallKey);
    }
    steps.extend(
        hardening_steps(&setup.hardening)
            .into_iter()
            .map(GuidedStep::Harden),
    );
    steps.push(GuidedStep::InstallNode);
    steps.push(GuidedStep::SetUpUpdates(setup.update_mode));
    if setup.access == SshAccessMode::ClientKeyOnly {
        steps.push(GuidedStep::RestrictSsh);
    }
    steps.push(GuidedStep::FetchInvite);
    steps
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepState {
    Running,
    Done(Vec<String>),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct GuidedProgress {
    /// Index into [`guided_steps`].
    pub index: usize,
    pub step: GuidedStep,
    pub state: StepState,
}

/// What the client keeps once the node is up.
#[derive(Debug, Clone)]
pub struct GuidedOutcome {
    /// A one-host inventory describing the node, for later management.
    pub inventory: Inventory,
    pub host_os: HostOs,
    /// `linux-x86_64` or `linux-aarch64`.
    pub platform: String,
    /// The node's public identity, to match against the live connection.
    pub node_identity: String,
    pub invite_url: String,
    /// The port sshd actually listens on (may differ behind forwarding).
    pub ssh_port: u16,
    pub report: Vec<String>,
}

/// Run every step, reporting each through `progress`. Stops at the first
/// failure, which is also returned.
pub async fn run_guided_setup(
    setup: &GuidedSetup,
    progress: &(dyn Fn(GuidedProgress) + Send + Sync),
) -> Result<GuidedOutcome> {
    setup.validate()?;
    let steps = guided_steps(setup);
    let mut runner = Runner {
        steps: &steps,
        progress,
        report: Vec::new(),
    };

    let mut transport = SshTransport::with_auth(&setup.ssh_target, setup.initial_auth());

    runner
        .step(GuidedStep::Connect, async {
            crate::ops::connect_host(&transport).await?;
            Ok(vec![format!("logged in to {}", setup.ssh_target)])
        })
        .await?;

    let host_os = runner
        .step_with(GuidedStep::CheckServer, async {
            let os = probe_host_os(&transport).await?;
            if let OsSupport::Unsupported(reason) = os.support() {
                bail!("{reason}");
            }
            if setup.update_mode == UpdateMode::ServerTimer && !os.reaches_github {
                bail!(
                    "this server cannot reach github.com, so it cannot fetch its own updates \
                     (GitHub has no IPv6 address, and the server seems to have no IPv4 \
                     internet access). Choose to update only when you click Update: \
                     DoubleSlash then downloads each update itself and uploads it"
                );
            }
            let mut lines = vec![format!("{} with systemd", os.display_name())];
            let others: Vec<_> = os
                .public_listeners
                .iter()
                .filter(|l| !is_ssh_listener(l, os.ssh_port))
                .cloned()
                .collect();
            if setup.hardening.firewall && !others.is_empty() {
                lines.push(format!(
                    "other services are listening ({}); the firewall will block them unless you allow them yourself",
                    others.join(", ")
                ));
            }
            Ok((os, lines))
        })
        .await?;
    let privilege = host_os
        .privilege()
        .context("no root access on the server")?;
    let ssh_port = host_os
        .ssh_port
        .unwrap_or_else(|| target_port(&setup.ssh_target));

    let mut key_proof = None;
    if setup.access.installs_client_key() {
        let key = setup
            .client_key
            .as_ref()
            .context("no client key to install")?;
        let proof = runner
            .step_with(GuidedStep::InstallKey, async {
                let public_line = key
                    .public_key()
                    .to_openssh()
                    .context("encode the client key")?;
                install_authorized_key(&transport, &public_line).await?;
                let proof =
                    verify_key_only_login(&setup.ssh_target, key.clone(), setup.host_key.clone())
                        .await?;
                Ok((
                    proof,
                    vec!["key installed and a key-only login worked".into()],
                ))
            })
            .await?;
        key_proof = Some(proof);
        transport = SshTransport::with_auth(&setup.ssh_target, setup.key_auth());
    }

    for step in hardening_steps(&setup.hardening) {
        runner
            .step(GuidedStep::Harden(step.clone()), async {
                apply_hardening_step(&transport, privilege, ssh_port, &step).await
            })
            .await?;
    }

    let inventory = build_inventory(setup, privilege);
    let platform = runner
        .step_with(GuidedStep::InstallNode, async {
            let resolved = resolve_one(&inventory)?;
            let platform = ping_host(&transport).await?.platform;
            let binary = resolve_install_binary(&transport, &resolved).await?;
            let mut lines =
                install_instance_report(&transport, &resolved, &binary.binary_path, None, false)
                    .await?;
            if let Some(sha) = &binary.archive_sha256 {
                lines.push(format!("package {}", &sha[..12]));
            }
            Ok(((platform, binary.archive_sha256), lines))
        })
        .await?;
    let (platform, archive_sha256) = platform;

    runner
        .step(GuidedStep::SetUpUpdates(setup.update_mode), async {
            let config = UpdaterConfig::from_defaults(&inventory.defaults, &platform)?;
            let lines = install_updater(&transport, privilege, &config, setup.update_mode).await?;
            if let Some(sha) = &archive_sha256 {
                record_installed_package(&transport, privilege, sha).await?;
            }
            Ok(lines)
        })
        .await?;

    if setup.access == SshAccessMode::ClientKeyOnly {
        let proof = key_proof.as_ref().context("key login was never verified")?;
        runner
            .step(GuidedStep::RestrictSsh, async {
                let mut lines = restrict_ssh_to_keys(&transport, privilege, proof).await?;
                // A fresh connection after the reload, so a lockout shows up
                // here and not the next time the user needs the server.
                let key = setup.client_key.clone().context("no client key")?;
                verify_key_only_login(&setup.ssh_target, key, setup.host_key.clone())
                    .await
                    .context("key login after turning passwords off")?;
                lines.push("key login still works".into());
                Ok(lines)
            })
            .await?;
    }

    let (node_identity, invite_url) = runner
        .step_with(GuidedStep::FetchInvite, async {
            let resolved = resolve_one(&inventory)?;
            let layout = InstanceLayout::from_resolved(&resolved);
            let label = instance_label(&resolved.host.name, resolved.instance);
            let (identity, invite) = wait_for_invite(&transport, &layout, &label).await?;
            Ok(((identity, invite), vec!["invite ready".into()]))
        })
        .await?;

    Ok(GuidedOutcome {
        inventory,
        host_os,
        platform,
        node_identity,
        invite_url,
        ssh_port,
        report: runner.report,
    })
}

struct Runner<'a> {
    steps: &'a [GuidedStep],
    progress: &'a (dyn Fn(GuidedProgress) + Send + Sync),
    report: Vec<String>,
}

impl Runner<'_> {
    async fn step(
        &mut self,
        step: GuidedStep,
        work: impl std::future::Future<Output = Result<Vec<String>>>,
    ) -> Result<()> {
        self.step_with(step, async { work.await.map(|lines| ((), lines)) })
            .await
    }

    async fn step_with<T>(
        &mut self,
        step: GuidedStep,
        work: impl std::future::Future<Output = Result<(T, Vec<String>)>>,
    ) -> Result<T> {
        let index = self
            .steps
            .iter()
            .position(|s| *s == step)
            .unwrap_or(usize::MAX);
        let emit = |state| {
            (self.progress)(GuidedProgress {
                index,
                step: step.clone(),
                state,
            })
        };
        emit(StepState::Running);
        match work.await {
            Ok((value, lines)) => {
                self.report.extend(lines.iter().cloned());
                emit(StepState::Done(lines));
                Ok(value)
            }
            Err(error) => {
                emit(StepState::Failed(format!("{error:#}")));
                Err(error.context(step.title()))
            }
        }
    }
}

fn build_inventory(setup: &GuidedSetup, privilege: snm_core::PrivilegeMode) -> Inventory {
    Inventory {
        defaults: Defaults {
            version: setup.channel.clone(),
            privilege,
            // Adds the node's ports when ufw is present, and only reports
            // them when it is not.
            firewall: FirewallMode::Ufw,
            ..Defaults::default()
        },
        host: vec![Host {
            name: setup.host_name.clone(),
            ssh: setup.ssh_target.clone(),
            arch: None,
            instances: vec![setup.instance.clone()],
        }],
        clusters: Vec::new(),
    }
}

fn resolve_one(inventory: &Inventory) -> Result<snm_core::ResolvedInstance<'_>> {
    inventory
        .resolve_instances(&Selector::from_flags(None, None, true))?
        .into_iter()
        .next()
        .context("the inventory has no instance")
}

/// A first start writes `identity.json` and then `reusable_invite.json`;
/// give it up to a minute.
async fn wait_for_invite(
    transport: &SshTransport,
    layout: &InstanceLayout,
    label: &str,
) -> Result<(String, String)> {
    let mut last_error = None;
    for _ in 0..30 {
        match (
            collect_identity_pub(transport, layout).await,
            fetch_invite(transport, layout, label).await,
        ) {
            (Ok(identity), Ok(invite)) => return Ok((identity, invite.invite_url)),
            (Err(e), _) | (_, Err(e)) => last_error = Some(e),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Err(last_error
        .unwrap_or_else(|| anyhow::anyhow!("no invite"))
        .context("the node did not write its invite; check its logs"))
}

fn target_port(target: &str) -> u16 {
    snm_transport::SshTarget::parse(target).port
}

fn is_ssh_listener(listener: &str, ssh_port: Option<u16>) -> bool {
    let port = listener
        .rsplit(':')
        .next()
        .and_then(|p| p.parse::<u16>().ok());
    listener.starts_with("tcp") && port.is_some() && port == ssh_port.or(Some(22))
}

#[cfg(test)]
mod tests {
    use super::*;
    use snm_transport::{generate_ed25519_key, parse_private_key, parse_public_key};

    fn setup(access: SshAccessMode, with_key: bool) -> GuidedSetup {
        let host = generate_ed25519_key("host").unwrap();
        let client = generate_ed25519_key("client").unwrap();
        GuidedSetup {
            host_name: "my-node".into(),
            ssh_target: "root@198.51.100.7".into(),
            public_host: "198.51.100.7".into(),
            host_key: parse_public_key(&host.public_openssh).unwrap(),
            access,
            client_key: with_key.then(|| parse_private_key(&client.private_openssh, None).unwrap()),
            password: Some("pw".into()),
            hardening: HardeningOptions::default(),
            update_mode: UpdateMode::ServerTimer,
            channel: "nightly".into(),
            instance: GuidedSetup::default_instance("198.51.100.7"),
        }
    }

    #[test]
    fn key_only_mode_restricts_ssh_after_install_and_before_invite() {
        let steps = guided_steps(&setup(SshAccessMode::ClientKeyOnly, true));
        let pos = |s: &GuidedStep| steps.iter().position(|x| x == s).unwrap();
        assert!(pos(&GuidedStep::InstallKey) < pos(&GuidedStep::InstallNode));
        assert!(pos(&GuidedStep::InstallNode) < pos(&GuidedStep::RestrictSsh));
        assert!(
            pos(&GuidedStep::SetUpUpdates(UpdateMode::ServerTimer)) < pos(&GuidedStep::RestrictSsh)
        );
        assert_eq!(steps.last(), Some(&GuidedStep::FetchInvite));
    }

    #[test]
    fn other_modes_never_touch_sshd() {
        for access in [
            SshAccessMode::ClientKeyAlongsidePassword,
            SshAccessMode::OwnKey,
        ] {
            let steps = guided_steps(&setup(access, true));
            assert!(!steps.contains(&GuidedStep::RestrictSsh));
        }
        let own = guided_steps(&setup(SshAccessMode::OwnKey, false));
        assert!(!own.contains(&GuidedStep::InstallKey));
    }

    #[test]
    fn key_only_mode_drops_the_password_once_the_key_works() {
        let s = setup(SshAccessMode::ClientKeyOnly, true);
        assert!(s.initial_auth().has_password());
        assert!(!s.key_auth().has_password());
        let s = setup(SshAccessMode::ClientKeyAlongsidePassword, true);
        assert!(s.key_auth().has_password());
    }

    #[test]
    fn client_key_modes_require_a_key() {
        assert!(setup(SshAccessMode::ClientKeyOnly, false)
            .validate()
            .is_err());
        assert!(setup(SshAccessMode::OwnKey, false).validate().is_ok());
    }

    #[test]
    fn inventory_pins_channel_ports_and_privilege() {
        let s = setup(SshAccessMode::ClientKeyOnly, true);
        let inventory = build_inventory(&s, snm_core::PrivilegeMode::Root);
        let resolved = resolve_one(&inventory).unwrap();
        assert_eq!(resolved.defaults.version, "nightly");
        assert_eq!(resolved.relay_port, 3478);
        assert_eq!(resolved.ws_port, 34935);
        assert_eq!(resolved.instance.public_host, "198.51.100.7");
        assert_eq!(resolved.defaults.privilege, snm_core::PrivilegeMode::Root);
    }

    #[test]
    fn setup_future_can_be_spawned() {
        fn require_send<T: Send>(_: &T) {}
        let s = setup(SshAccessMode::ClientKeyOnly, true);
        let future = run_guided_setup(&s, &|_| {});
        require_send(&future);
    }

    #[test]
    fn recognises_the_ssh_listener() {
        assert!(is_ssh_listener("tcp 0.0.0.0:22", Some(22)));
        assert!(is_ssh_listener("tcp [::]:2222", Some(2222)));
        assert!(!is_ssh_listener("tcp 0.0.0.0:80", Some(22)));
        assert!(!is_ssh_listener("udp 0.0.0.0:22", Some(22)));
    }
}
