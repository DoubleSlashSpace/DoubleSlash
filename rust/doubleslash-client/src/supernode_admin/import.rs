//! Local adoption of read-only server discovery snapshots.
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use snm_core::{Defaults, Host, Inventory};
use snm_supernode::{ExistingInstance, GuidedSetup};
use snm_transport::{parse_public_key, PrivateKey, SessionAuth, SshTransport};
use std::collections::{BTreeMap, HashSet};
use zeroize::Zeroize;

use super::service::{bracket, ssh_host};
use super::store::{ManagedHost, ManagedNode};

#[derive(Default, Deserialize)]
pub struct ImportCredentials {
    #[serde(default)]
    pub ssh_password: String,
}

impl Drop for ImportCredentials {
    fn drop(&mut self) {
        self.ssh_password.zeroize();
    }
}

#[derive(Deserialize)]
pub struct ImportServerRequest {
    #[serde(default)]
    pub name: String,
    pub host: String,
    #[serde(default = "default_user")]
    pub user: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub host_key: String,
    #[serde(flatten)]
    pub credentials: ImportCredentials,
}

fn default_user() -> String {
    "root".into()
}
fn default_port() -> u16 {
    22
}

#[derive(Clone)]
pub(super) struct ImportConnection {
    pub name: String,
    pub ssh_target: String,
    pub host_key: String,
}

impl ImportServerRequest {
    pub(super) fn connection(
        &self,
        key: Option<&PrivateKey>,
    ) -> Result<(ImportConnection, SshTransport), String> {
        let host = ssh_host(&self.host).ok_or("Enter the existing server's SSH address")?;
        let user = self.user.trim();
        if user.is_empty()
            || !user
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || self.port == 0
        {
            return Err("Enter a valid SSH login name and port".into());
        }
        let public = parse_public_key(&self.host_key)
            .map_err(|_| "Confirm this server's SSH fingerprint first")?;
        let mut auth = SessionAuth::pinned(public).with_default_identity_files();
        if let Some(key) = key {
            auth = auth.with_key(key.clone());
        }
        if !self.credentials.ssh_password.is_empty() {
            auth = auth.with_password(self.credentials.ssh_password.clone());
        }
        let ssh_target = format!("{user}@{}:{}", bracket(host), self.port);
        Ok((
            ImportConnection {
                name: crate::peer_store::clean_local_name(&self.name),
                ssh_target: ssh_target.clone(),
                host_key: self.host_key.clone(),
            },
            SshTransport::with_auth(&ssh_target, auth),
        ))
    }
}

pub(super) struct PendingImport {
    pub id: String,
    pub created_at: u64,
    pub connection: ImportConnection,
    pub instances: Vec<ExistingInstance>,
}

impl PendingImport {
    pub fn preview(&self) -> Value {
        json!({ "discovery_id": self.id, "instances": self.instances.iter().map(|i| json!({
            "unit": i.unit, "instance_id": i.instance_id, "public_host": i.public_host,
            "ws_port": i.ws_addr.port(), "relay_port": i.relay_addr.port(), "data_dir": i.data_dir,
            "active": i.active, "cluster_id": i.cluster.as_ref().map(|c| &c.cluster_id),
            "cluster_members": i.cluster.as_ref().map(|c| c.members.len()).unwrap_or(1),
        })).collect::<Vec<_>>() })
    }

    pub fn nodes(&self, units: &[String], now: u64) -> Result<Vec<ManagedNode>, String> {
        let selected: HashSet<_> = units.iter().map(String::as_str).collect();
        if selected.is_empty()
            || selected.len() != units.len()
            || selected
                .iter()
                .any(|unit| !self.instances.iter().any(|i| i.unit == *unit))
        {
            return Err("Select existing instances from this discovery result".into());
        }
        let mut groups: BTreeMap<String, Vec<&ExistingInstance>> = BTreeMap::new();
        for instance in &self.instances {
            if selected.contains(instance.unit.as_str()) {
                groups
                    .entry(instance.group_key())
                    .or_default()
                    .push(instance);
            }
        }
        let mut nodes = Vec::new();
        for (group, instances) in groups {
            let first = instances[0];
            let cluster_id = first.cluster.as_ref().map(|c| c.cluster_id.clone());
            let name = if self.connection.name.is_empty() {
                cluster_id
                    .clone()
                    .unwrap_or_else(|| format!("Imported {}", first.instance_id))
            } else {
                self.connection.name.clone()
            };
            let mut hosts = Vec::new();
            let mut inventory = Inventory {
                defaults: Defaults::default(),
                host: Vec::new(),
                clusters: Vec::new(),
            };
            for instance in instances {
                let alias = format!(
                    "import-{:x}",
                    Sha256::digest(
                        format!(
                            "{}\n{}\n{}",
                            self.connection.ssh_target, self.connection.host_key, instance.unit
                        )
                        .as_bytes()
                    )
                );
                let host_name = alias[..23].to_owned();
                hosts.push(ManagedHost {
                    host_name: host_name.clone(),
                    ssh_target: self.connection.ssh_target.clone(),
                    host_key: self.connection.host_key.clone(),
                    access: "existing".into(),
                    public_host: bracket(&instance.public_host),
                    platform: "Imported Linux service".into(),
                    node_identity: instance.public_id.clone(),
                    os: "Linux (existing deployment)".into(),
                    existing: Some(instance.clone()),
                });
                inventory.host.push(Host {
                    name: host_name,
                    ssh: self.connection.ssh_target.clone(),
                    arch: None,
                    instances: vec![proxy_instance(instance)],
                });
            }
            nodes.push(ManagedNode {
                id: format!("imported-{}", &group[..24]),
                name,
                channel: "existing".into(),
                update_mode: "external".into(),
                cluster_id,
                invite_url: first.invite_url.clone(),
                created_at: now,
                hosts,
                inventory,
            });
        }
        Ok(nodes)
    }
}

pub(super) fn proxy_instance(existing: &ExistingInstance) -> snm_core::Instance {
    let mut instance = GuidedSetup::default_instance(&bracket(&existing.public_host));
    instance.id = existing.instance_id.clone();
    instance.hostname = existing.hostname.clone();
    instance.hostname_lookup = existing.hostname_lookup.clone();
    instance.relay_port = Some(existing.relay_addr.port());
    instance.ws_port = Some(existing.ws_addr.port());
    instance.listen_bind = Some(existing.ws_addr.ip().to_string());
    instance
}

pub(super) fn is_imported(node: &ManagedNode) -> bool {
    node.hosts.iter().any(|h| h.existing.is_some())
}

pub(super) fn merge_imported(node: &mut ManagedNode, imported: ManagedNode) {
    for mut host in imported.hosts {
        let mut inventory_host = imported
            .inventory
            .host
            .iter()
            .find(|h| h.name == host.host_name)
            .cloned();
        if let Some(existing) = node.hosts.iter_mut().find(|h| {
            h.node_identity.trim_end_matches('=') == host.node_identity.trim_end_matches('=')
        }) {
            host.host_name = existing.host_name.clone();
            *existing = host.clone();
        } else {
            node.hosts.push(host.clone());
        }
        if let Some(mut updated) = inventory_host.take() {
            updated.name = host.host_name.clone();
            if let Some(previous) = node
                .inventory
                .host
                .iter_mut()
                .find(|h| h.name == updated.name)
            {
                *previous = updated;
            } else {
                node.inventory.host.push(updated);
            }
        }
    }
    if node.invite_url.is_empty() {
        node.invite_url = imported.invite_url;
    }
}

#[cfg(test)]
pub(super) fn fixture(instances: &[usize]) -> PendingImport {
    use snm_supernode::{ExistingCluster, ExistingMember};
    let roster = ExistingCluster {
        cluster_id: "operator-cluster".into(),
        members: (0..3)
            .map(|i| ExistingMember {
                identity_pub: format!("public-node-{i}"),
                relay_addr: format!("198.51.100.{}:{}", 7 + i / 2, 42000 + i),
                cluster_addr: format!("198.51.100.{}:{}", 7 + i / 2, 44000 + i),
                ws_addr: format!("ws://198.51.100.{}:{}", 7 + i / 2, 41000 + i),
            })
            .collect(),
    };
    PendingImport {
        id: "discovery".into(),
        created_at: 0,
        connection: ImportConnection {
            name: "Imported cluster".into(),
            ssh_target: format!(
                "root@198.51.100.{}:22",
                if instances.contains(&2) { 8 } else { 7 }
            ),
            host_key: snm_transport::generate_ed25519_key("host")
                .unwrap()
                .public_openssh,
        },
        instances: instances
            .iter()
            .map(|i| ExistingInstance {
                unit: format!("doubleslash-supernode@{}.service", ['a', 'b', 'a'][*i]),
                instance_id: ['a', 'b', 'a'][*i].to_string(),
                data_dir: format!("/srv/custom/instance-{i}"),
                manifest_sha256: "hash".into(),
                executable: "/srv/bin/supernode".into(),
                service_user: "custom-user".into(),
                privilege: snm_core::PrivilegeMode::Root,
                public_id: format!("public-node-{i}"),
                public_host: format!("198.51.100.{}", 7 + i / 2),
                relay_addr: format!("[::]:{}", 42000 + i).parse().unwrap(),
                ws_addr: format!("0.0.0.0:{}", 41000 + i).parse().unwrap(),
                hostname: None,
                hostname_lookup: None,
                cluster: Some(roster.clone()),
                invite_url: String::new(),
                active: true,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn co_located_instances_have_independent_targets_and_real_ports() {
        let pending = fixture(&[0, 1]);
        let units: Vec<_> = pending.instances.iter().map(|i| i.unit.clone()).collect();
        let nodes = pending.nodes(&units, 1).unwrap();
        assert_eq!(nodes.len(), 1);
        let node = &nodes[0];
        assert_eq!(node.hosts.len(), 2);
        assert_ne!(node.hosts[0].host_name, node.hosts[1].host_name);
        assert_eq!(node.hosts[0].ssh_target, node.hosts[1].ssh_target);
        assert_eq!(node.inventory.host[1].instances[0].ws_port, Some(41001));
        assert!(
            node.inventory.clusters.is_empty(),
            "descriptive inventory cannot be cluster-synced"
        );
        assert!(!pending.preview().to_string().contains("manifest_sha256"));
        assert!(pending.nodes(&[], 1).is_err());
        assert!(pending
            .nodes(&[units[0].clone(), units[0].clone()], 1)
            .is_err());
        assert!(pending.nodes(&["unknown".into()], 1).is_err());
    }

    #[test]
    fn second_vps_merges_idempotently_without_losing_local_name() {
        let first = fixture(&[0, 1]);
        let mut node = first
            .nodes(
                &first
                    .instances
                    .iter()
                    .map(|i| i.unit.clone())
                    .collect::<Vec<_>>(),
                1,
            )
            .unwrap()
            .remove(0);
        node.name = "My local name".into();
        let mut second = fixture(&[2]);
        // Cloned VPS images can share an SSH host key and instance id.
        // The SSH endpoint still has to distinguish their local targets.
        second.connection.host_key = first.connection.host_key.clone();
        let imported = second
            .nodes(&[second.instances[0].unit.clone()], 2)
            .unwrap()
            .remove(0);
        assert_eq!(node.id, imported.id);
        merge_imported(&mut node, imported.clone());
        merge_imported(&mut node, imported);
        assert_eq!(node.hosts.len(), 3);
        assert_eq!(node.inventory.host.len(), 3);
        assert_eq!(node.name, "My local name");
        assert_ne!(node.hosts[0].ssh_target, node.hosts[2].ssh_target);
        assert_ne!(node.hosts[0].host_name, node.hosts[2].host_name);
        assert_eq!(node.hosts[2].existing.as_ref().unwrap().instance_id, "a");
    }

    #[test]
    fn same_cluster_label_with_different_roster_never_groups() {
        let mut pending = fixture(&[0, 1]);
        pending.instances[1].cluster.as_mut().unwrap().members.pop();
        let nodes = pending
            .nodes(
                &pending
                    .instances
                    .iter()
                    .map(|i| i.unit.clone())
                    .collect::<Vec<_>>(),
                1,
            )
            .unwrap();
        assert_eq!(nodes.len(), 2);
        assert_ne!(nodes[0].id, nodes[1].id);
    }

    #[test]
    fn import_credentials_are_not_in_connection_metadata() {
        let key = snm_transport::generate_ed25519_key("host")
            .unwrap()
            .public_openssh;
        let request:ImportServerRequest = serde_json::from_value(json!({"host":"[2001:db8::7]","host_key":key,"ssh_password":"secret","name":"  Fleet\n  "})).unwrap();
        let (connection, _) = request.connection(None).unwrap();
        assert_eq!(connection.ssh_target, "root@[2001:db8::7]:22");
        assert_eq!(connection.name, "Fleet");
        // Neither the preview nor the persisted node has a credential field.
        let mut pending = fixture(&[0]);
        pending.connection = connection;
        let nodes = pending
            .nodes(&[pending.instances[0].unit.clone()], 1)
            .unwrap();
        let stored = serde_json::to_string(&nodes).unwrap();
        assert!(!stored.contains("secret"));
        assert!(!stored.contains("ssh_password"));
    }
}
