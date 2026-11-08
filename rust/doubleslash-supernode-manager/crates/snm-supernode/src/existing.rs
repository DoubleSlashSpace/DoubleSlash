//! Read-only adoption of existing instances; hostname-only edits are separate.
use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snm_core::PrivilegeMode;
use snm_transport::{shell_escape, Transport};

const PROBE: &str = include_str!("existing_probe.py");
const MAX_SNAPSHOT: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExistingMember {
    pub identity_pub: String,
    pub relay_addr: String,
    pub cluster_addr: String,
    pub ws_addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExistingCluster {
    pub cluster_id: String,
    #[serde(default, rename = "member")]
    pub members: Vec<ExistingMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExistingInstance {
    pub unit: String,
    pub instance_id: String,
    pub data_dir: String,
    pub manifest_sha256: String,
    pub executable: String,
    pub service_user: String,
    pub privilege: PrivilegeMode,
    pub public_id: String,
    pub public_host: String,
    pub relay_addr: SocketAddr,
    pub ws_addr: SocketAddr,
    pub hostname: Option<String>,
    pub hostname_lookup: Option<String>,
    pub cluster: Option<ExistingCluster>,
    pub invite_url: String,
    pub active: bool,
}

impl ExistingInstance {
    pub fn group_key(&self) -> String {
        let mut ids = self
            .cluster
            .as_ref()
            .map(|c| {
                c.members
                    .iter()
                    .map(|m| m.identity_pub.trim_end_matches('=').to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![self.public_id.trim_end_matches('=').to_owned()]);
        ids.sort();
        let group = self
            .cluster
            .as_ref()
            .map(|c| c.cluster_id.as_str())
            .unwrap_or("standalone");
        hex_digest(format!("{group}\n{}", ids.join("\n")).as_bytes())
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_unit(unit: &str) -> bool {
    unit.strip_prefix("doubleslash-supernode@")
        .and_then(|s| s.strip_suffix(".service"))
        .is_some_and(|id| {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
}

pub async fn has_existing_supernodes(transport: &impl Transport) -> Result<bool> {
    let mut found = false;
    for command in [
        "systemctl list-units --all --type=service --plain --no-legend --no-pager 'doubleslash-supernode@*.service'",
        "systemctl list-unit-files --type=service --no-legend --no-pager 'doubleslash-supernode@*.service'",
    ] {
        let output = transport.run(command).await?;
        ensure!(output.exit_code == 0, "Could not check for an existing supernode deployment");
        found |= output.stdout.lines().filter_map(|l| l.split_whitespace().next()).any(valid_unit);
    }
    Ok(found)
}

pub async fn inspect_existing_supernodes(
    transport: &impl Transport,
) -> Result<Vec<ExistingInstance>> {
    let output = transport.run("id -u").await?;
    ensure!(output.exit_code == 0, "Could not check SSH permissions");
    let privilege = if output.stdout.trim() == "0" {
        PrivilegeMode::Root
    } else {
        ensure!(
            transport.run("sudo -n true").await?.exit_code == 0,
            "Import needs root or passwordless sudo to read the running instances"
        );
        PrivilegeMode::Sudo
    };
    let prefix = if privilege == PrivilegeMode::Root {
        ""
    } else {
        "sudo -n "
    };
    let command = format!("{prefix}timeout 30s python3 -c {}", shell_escape(PROBE));
    let output = tokio::time::timeout(Duration::from_secs(35), transport.run(&command))
        .await
        .context("Existing-instance discovery timed out")??;
    ensure!(output.exit_code == 0, "Could not inspect existing instances: Python 3, systemd and data-directory read access are required");
    ensure!(
        output.stdout.len() <= MAX_SNAPSHOT,
        "Existing-instance snapshot is too large"
    );
    parse_snapshot(&output.stdout, privilege)
}

#[derive(Deserialize)]
struct RawInstance {
    unit: String,
    home: String,
    public_key: String,
    public_host: String,
    relay_port: String,
    ws_port: String,
    user: String,
    executable: String,
    active: bool,
    manifest: String,
    invite: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Manifest {
    schema_version: u32,
    listen_addr: Option<SocketAddr>,
    ws_listen_addr: Option<SocketAddr>,
    hostname: Option<String>,
    hostname_lookup: Option<String>,
    cluster: Option<ExistingCluster>,
}

fn parse_snapshot(raw: &str, privilege: PrivilegeMode) -> Result<Vec<ExistingInstance>> {
    #[derive(Deserialize)]
    struct Snapshot {
        instances: Vec<RawInstance>,
    }
    let snapshot: Snapshot =
        serde_json::from_str(raw).context("Invalid existing-instance snapshot")?;
    ensure!(
        !snapshot.instances.is_empty(),
        "No existing DoubleSlash supernode instances were found"
    );
    ensure!(
        snapshot.instances.len() <= 64,
        "Too many existing instances"
    );
    let mut result = Vec::new();
    let mut units = HashSet::new();
    let mut identities = HashSet::new();
    let mut directories = HashSet::new();
    for raw in snapshot.instances {
        ensure!(valid_unit(&raw.unit), "Unsupported service unit name");
        ensure!(
            units.insert(raw.unit.clone()) && directories.insert(raw.home.clone()),
            "Duplicate instance unit or data directory"
        );
        ensure!(
            raw.home.starts_with('/') && !raw.home.split('/').any(|p| p == ".."),
            "Invalid instance data directory"
        );
        let public_id = raw.public_key.trim_end_matches('=').to_owned();
        ensure!(
            identities.insert(public_id.clone()),
            "Multiple instances share a node identity; import refused"
        );
        ensure!(
            URL_SAFE_NO_PAD
                .decode(&public_id)
                .is_ok_and(|key| key.len() == 32),
            "Invalid public node identity"
        );
        let manifest: Manifest = toml::from_str(&raw.manifest)
            .map_err(|_| anyhow::anyhow!("Cannot parse existing supernode.toml"))?;
        ensure!(
            manifest.schema_version == 1,
            "Unsupported existing supernode manifest schema"
        );
        if let Some(cluster) = &manifest.cluster {
            let ids: HashSet<_> = cluster
                .members
                .iter()
                .map(|m| m.identity_pub.trim_end_matches('='))
                .collect();
            ensure!(
                !cluster.cluster_id.is_empty()
                    && ids.len() == cluster.members.len()
                    && ids.len() <= 256
                    && ids
                        .iter()
                        .all(|id| URL_SAFE_NO_PAD.decode(id).is_ok_and(|key| key.len() == 32)),
                "Invalid existing cluster roster"
            );
            ensure!(
                cluster
                    .members
                    .iter()
                    .any(|m| m.identity_pub.trim_end_matches('=') == public_id),
                "The instance is missing from its own cluster roster"
            );
        }
        let public_host: IpAddr = raw
            .public_host
            .trim_matches(['[', ']'])
            .parse()
            .context("Existing supernode_host must be an IP literal")?;
        let relay_addr = match manifest.listen_addr {
            Some(addr) => addr,
            None => SocketAddr::new("0.0.0.0".parse()?, raw.relay_port.parse()?),
        };
        let ws_addr = match manifest.ws_listen_addr {
            Some(addr) => addr,
            None => SocketAddr::new("0.0.0.0".parse()?, raw.ws_port.parse()?),
        };
        ensure!(
            relay_addr.port() != 0 && ws_addr.port() != 0,
            "Import requires fixed relay and signaling ports"
        );
        let invite_url = raw
            .invite
            .as_ref()
            .and_then(|invite| {
                crate::invite::parse_reusable_invite(
                    &serde_json::json!({"invite":invite}).to_string(),
                )
                .ok()
            })
            .unwrap_or_default();
        let instance_id = raw
            .unit
            .trim_start_matches("doubleslash-supernode@")
            .trim_end_matches(".service")
            .to_owned();
        result.push(ExistingInstance {
            unit: raw.unit,
            instance_id,
            data_dir: raw.home,
            manifest_sha256: hex_digest(raw.manifest.as_bytes()),
            executable: raw.executable,
            service_user: raw.user,
            privilege,
            public_id,
            public_host: public_host.to_string(),
            relay_addr,
            ws_addr,
            hostname: manifest.hostname,
            hostname_lookup: manifest.hostname_lookup,
            cluster: manifest.cluster,
            invite_url,
            active: raw.active,
        });
    }
    Ok(result)
}

pub async fn existing_status(
    transport: &impl Transport,
    instance: &ExistingInstance,
) -> Result<String> {
    ensure!(valid_unit(&instance.unit), "Invalid imported service unit");
    let output = transport
        .run(&format!(
            "systemctl show --property=ActiveState --value -- {}",
            shell_escape(&instance.unit)
        ))
        .await?;
    ensure!(
        output.exit_code == 0,
        "Could not read imported instance status"
    );
    Ok(output.stdout.trim().to_owned())
}

pub async fn existing_logs(
    transport: &impl Transport,
    instance: &ExistingInstance,
    lines: u32,
) -> Result<String> {
    ensure!(valid_unit(&instance.unit), "Invalid imported service unit");
    let prefix = if instance.privilege == PrivilegeMode::Root {
        ""
    } else {
        "sudo -n "
    };
    let output = transport
        .run(&format!(
            "{prefix}journalctl --no-pager -n {} -u {}",
            lines.min(2000),
            shell_escape(&instance.unit)
        ))
        .await?;
    ensure!(
        output.exit_code == 0,
        "Could not read imported instance logs"
    );
    Ok(output.stdout)
}

pub async fn restart_existing(
    transport: &impl Transport,
    instance: &ExistingInstance,
) -> Result<()> {
    ensure!(valid_unit(&instance.unit), "Invalid imported service unit");
    let prefix = if instance.privilege == PrivilegeMode::Root {
        ""
    } else {
        "sudo -n "
    };
    ensure!(
        transport
            .run(&format!(
                "{prefix}systemctl restart -- {}",
                shell_escape(&instance.unit)
            ))
            .await?
            .exit_code
            == 0,
        "Hostname was saved, but restarting this instance failed"
    );
    Ok(())
}

/// Change only root hostname fields, retaining all other TOML bytes/comments.
pub fn hostname_only_patch(
    raw: &str,
    hostname: Option<&str>,
    lookup: Option<&str>,
) -> Result<String> {
    let mut document: toml_edit::DocumentMut = raw
        .parse()
        .map_err(|_| anyhow::anyhow!("Cannot parse existing manifest for hostname edit"))?;
    match hostname {
        Some(name) => {
            document["hostname"] = toml_edit::value(name);
        }
        None => {
            document.remove("hostname");
        }
    }
    match lookup {
        Some(mode) if hostname.is_some() => {
            document["hostname_lookup"] = toml_edit::value(mode);
        }
        _ => {
            document.remove("hostname_lookup");
        }
    }
    Ok(document.to_string())
}

/// This is invoked only by explicit hostname Apply, never by discovery/import.
pub async fn update_existing_hostname(
    transport: &impl Transport,
    expected: &ExistingInstance,
    hostname: Option<&str>,
    lookup: Option<&str>,
) -> Result<ExistingInstance> {
    let mut fresh = inspect_existing_supernodes(transport)
        .await?
        .into_iter()
        .find(|instance| instance.unit == expected.unit)
        .context("Imported service is no longer present")?;
    ensure!(fresh.public_id == expected.public_id && fresh.data_dir == expected.data_dir,
        "The imported instance's identity or data directory changed; import it again before editing");
    ensure!(
        fresh.group_key() == expected.group_key(),
        "The cluster roster changed; import it again before editing"
    );
    let prefix = if fresh.privilege == PrivilegeMode::Root {
        ""
    } else {
        "sudo -n "
    };
    let path = format!("{}/supernode.toml", fresh.data_dir);
    let output = transport
        .run(&format!("{prefix}cat -- {}", shell_escape(&path)))
        .await?;
    ensure!(
        output.exit_code == 0 && output.stdout.len() <= 262144,
        "Cannot read the existing manifest"
    );
    ensure!(
        hex_digest(output.stdout.as_bytes()) == fresh.manifest_sha256,
        "The manifest changed during this edit; retry"
    );
    let patch = hostname_only_patch(&output.stdout, hostname, lookup)?;
    let hash = hex_digest(output.stdout.as_bytes());
    let stage = format!("/tmp/doubleslash-hostname-{}.toml", stage_nonce()?);
    transport
        .upload_bytes(&stage, patch.as_bytes(), 0o600)
        .await?;
    let script = include_str!("existing_hostname.py");
    let command = format!(
        "{prefix}python3 -c {} {} {} {} {}",
        shell_escape(script),
        shell_escape(&path),
        shell_escape(&stage),
        shell_escape(&hash),
        shell_escape(&fresh.public_id)
    );
    let output = transport.run(&command).await?;
    ensure!(
        output.exit_code == 0,
        "Hostname edit refused or failed; the existing file may have changed. Re-import and retry"
    );
    fresh.hostname = hostname.map(str::to_owned);
    fresh.hostname_lookup = lookup.filter(|_| hostname.is_some()).map(str::to_owned);
    fresh.manifest_sha256 = hex_digest(patch.as_bytes());
    Ok(fresh)
}

fn stage_nonce() -> Result<String> {
    let mut random = [0u8; 24];
    getrandom::getrandom(&mut random)
        .map_err(|_| anyhow::anyhow!("Could not create a secure staging name"))?;
    Ok(URL_SAFE_NO_PAD.encode(random))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;
    use snm_transport::{RemoteOutput, TransportError};
    use std::sync::Mutex;

    const CONFIG: &str = r#"# operator-owned configuration
schema_version = 1
listen_addr = "[::]:42001"
ws_listen_addr = "0.0.0.0:41001" # intentionally split binds
hostname = "old.example.com"
hostname_lookup = "dns"
access_mode = "tos"
custom_setting = "retain me"

[[feature]]
id = "room.audio.sfu"
enabled = true
params = { allow_public_rooms = false, custom = 77 }
"#;

    fn snapshot(config: &str) -> String {
        json!({"instances":[{
            "unit":"doubleslash-supernode@b.service", "home":"/srv/custom/second",
            "public_key":URL_SAFE_NO_PAD.encode([7u8;32]), "public_host":"198.51.100.7",
            "relay_port":"3478", "ws_port":"34935", "user":"operator",
            "executable":"/srv/bin/custom-supernode", "active":true,
            "manifest":config,"invite":null
        }]})
        .to_string()
    }

    struct Fake {
        snapshot: String,
        config: String,
        writes_fail: bool,
        commands: Mutex<Vec<String>>,
        uploads: Mutex<Vec<(String, Vec<u8>, u32)>>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                snapshot: snapshot(CONFIG),
                config: CONFIG.into(),
                writes_fail: false,
                commands: Mutex::default(),
                uploads: Mutex::default(),
            }
        }
    }

    #[async_trait]
    impl Transport for Fake {
        async fn run(&self, command: &str) -> std::result::Result<RemoteOutput, TransportError> {
            self.commands.lock().unwrap().push(command.into());
            let stdout = if command == "id -u" {
                "0".to_owned()
            } else if command.starts_with("timeout 30s python3 -c ") {
                self.snapshot.clone()
            } else if command.starts_with("cat -- ") {
                self.config.clone()
            } else if command.contains("list-unit") {
                "doubleslash-supernode@b.service enabled".into()
            } else if command.starts_with("python3 -c ")
                || command.starts_with("systemctl restart -- ")
            {
                String::new()
            } else {
                return Err(TransportError::Other(format!(
                    "Unexpected command: {command}"
                )));
            };
            Ok(RemoteOutput {
                stdout,
                stderr: String::new(),
                exit_code: if self.writes_fail && command.starts_with("python3 -c ") {
                    1
                } else {
                    0
                },
            })
        }

        async fn upload_bytes(
            &self,
            path: &str,
            contents: &[u8],
            mode: u32,
        ) -> std::result::Result<(), TransportError> {
            self.uploads
                .lock()
                .unwrap()
                .push((path.into(), contents.to_vec(), mode));
            Ok(())
        }
    }

    #[tokio::test]
    async fn discovery_and_setup_preflight_are_read_only_and_use_real_ports() {
        let remote = Fake::new();
        assert!(has_existing_supernodes(&remote).await.unwrap());
        let instances = inspect_existing_supernodes(&remote).await.unwrap();
        assert_eq!(instances.len(), 1);
        let instance = &instances[0];
        assert_eq!(instance.ws_addr.to_string(), "0.0.0.0:41001");
        assert_eq!(instance.relay_addr.to_string(), "[::]:42001");
        assert_eq!(instance.data_dir, "/srv/custom/second");
        assert_eq!(instance.instance_id, "b");
        assert!(remote.uploads.lock().unwrap().is_empty());
        let commands = remote.commands.lock().unwrap();
        assert_eq!(commands.len(), 4);
        assert!(commands[0].starts_with("systemctl list-units "));
        assert!(commands[1].starts_with("systemctl list-unit-files "));
        assert_eq!(commands[2], "id -u");
        assert_eq!(
            commands[3],
            format!("timeout 30s python3 -c {}", shell_escape(PROBE))
        );
        // The probe reads identity.json remotely and exports only its public key.
        assert!(!PROBE.contains("private_key"));
        assert!(!PROBE.contains("open(path, \"w"));
    }

    #[test]
    fn hostname_patch_preserves_custom_features_split_binds_and_cluster_bytes() {
        let config = format!("{CONFIG}\n# retained roster\n[cluster]\ncluster_id = 'custom'\n[[cluster.member]]\nidentity_pub = 'public'\nrelay_addr = '198.51.100.7:42001'\n");
        for (name, lookup) in [(Some("new.example.com"), Some("dddns")), (None, None)] {
            let patch = hostname_only_patch(&config, name, lookup).unwrap();
            let mut before: toml::Value = toml::from_str(&config).unwrap();
            let mut after: toml::Value = toml::from_str(&patch).unwrap();
            for field in ["hostname", "hostname_lookup"] {
                before.as_table_mut().unwrap().remove(field);
                after.as_table_mut().unwrap().remove(field);
            }
            assert_eq!(before, after);
            assert!(
                patch.contains("ws_listen_addr = \"0.0.0.0:41001\" # intentionally split binds")
            );
            assert_eq!(
                config.split("# retained roster").nth(1),
                patch.split("# retained roster").nth(1)
            );
        }
    }

    #[test]
    fn first_hostname_is_added_at_the_root_without_changing_feature_tables() {
        let config = CONFIG
            .lines()
            .filter(|line| !line.starts_with("hostname"))
            .collect::<Vec<_>>()
            .join("\n");
        let patch = hostname_only_patch(&config, Some("new.example.com"), Some("dddns")).unwrap();
        let before: toml::Value = toml::from_str(&config).unwrap();
        let mut after: toml::Value = toml::from_str(&patch).unwrap();
        assert_eq!(after["hostname"].as_str(), Some("new.example.com"));
        assert_eq!(after["hostname_lookup"].as_str(), Some("dddns"));
        after.as_table_mut().unwrap().remove("hostname");
        after.as_table_mut().unwrap().remove("hostname_lookup");
        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn explicit_hostname_apply_stages_one_minimal_patch_and_restart_is_separate() {
        let remote = Fake::new();
        let expected = parse_snapshot(&remote.snapshot, PrivilegeMode::Root)
            .unwrap()
            .remove(0);
        let fresh = update_existing_hostname(&remote, &expected, Some("new.example.com"), None)
            .await
            .unwrap();
        assert_eq!(fresh.ws_addr.port(), 41001);
        {
            let uploads = remote.uploads.lock().unwrap();
            assert_eq!(uploads.len(), 1);
            assert!(uploads[0].0.starts_with("/tmp/doubleslash-hostname-"));
            assert_eq!(uploads[0].2, 0o600);
            assert!(std::str::from_utf8(&uploads[0].1)
                .unwrap()
                .contains("custom_setting = \"retain me\""));
        }
        assert!(!remote
            .commands
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.contains("systemctl restart")));
        restart_existing(&remote, &fresh).await.unwrap();
        assert_eq!(
            remote.commands.lock().unwrap().last().unwrap(),
            "systemctl restart -- doubleslash-supernode@b.service"
        );
    }

    #[tokio::test]
    async fn changed_identity_directory_or_manifest_refuses_before_upload() {
        let mut remote = Fake::new();
        let expected = parse_snapshot(&remote.snapshot, PrivilegeMode::Root)
            .unwrap()
            .remove(0);
        let mut replaced = expected.clone();
        replaced.public_id = URL_SAFE_NO_PAD.encode([8u8; 32]);
        assert!(update_existing_hostname(&remote, &replaced, None, None)
            .await
            .is_err());
        replaced = expected.clone();
        replaced.data_dir = "/srv/another".into();
        assert!(update_existing_hostname(&remote, &replaced, None, None)
            .await
            .is_err());
        remote.config.push_str("\n# concurrent edit\n");
        assert!(update_existing_hostname(&remote, &expected, None, None)
            .await
            .is_err());
        assert!(remote.uploads.lock().unwrap().is_empty());
        assert!(!remote
            .commands
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.contains("systemctl restart")));
    }

    #[tokio::test]
    async fn failed_patch_never_restarts() {
        let mut remote = Fake::new();
        remote.writes_fail = true;
        let expected = parse_snapshot(&remote.snapshot, PrivilegeMode::Root)
            .unwrap()
            .remove(0);
        assert!(update_existing_hostname(&remote, &expected, None, None)
            .await
            .is_err());
        assert!(!remote
            .commands
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.contains("systemctl restart")));
    }

    #[test]
    fn refuses_ambiguous_instances_and_invalid_rosters() {
        let raw = snapshot(CONFIG);
        let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let duplicate = value["instances"][0].clone();
        value["instances"].as_array_mut().unwrap().push(duplicate);
        assert!(parse_snapshot(&value.to_string(), PrivilegeMode::Root).is_err());
        let invalid = format!("{CONFIG}\n[cluster]\ncluster_id = 'c'\n[[cluster.member]]\nidentity_pub='bad'\nrelay_addr='a'\ncluster_addr='a'\nws_addr='a'\n");
        assert!(parse_snapshot(&snapshot(&invalid), PrivilegeMode::Root).is_err());
        assert!(!valid_unit("doubleslash-supernode@a.service; reboot"));
    }
}
