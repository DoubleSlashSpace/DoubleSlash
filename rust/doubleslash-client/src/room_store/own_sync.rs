//! Room definitions shared between one identity's own devices.
//!
//! A room is written to the store of the device that created or joined it, so
//! a room made on the phone never appeared on the desktop, and the reverse.
//! The connection manager carries an [`OwnRoomSnapshot`] between our devices,
//! sealed to our own identity, and each side merges what it lacks.
//!
//! Merging only adds. A stored room is never removed, renamed or retyped here,
//! and sidebar hiding stays with the device that hid it: hidden rooms are left
//! out of a snapshot, and a room hidden here is not brought back by one.
//!
//! Space trees merge by node, and the result is re-signed at a new epoch when
//! it matches neither side. Both devices sign roots for the same Space, so if
//! each kept its own tree they would announce different roots for one epoch.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::{RoomEntry, RoomStore};
use crate::error::Result;
use crate::space::{derive_node_id, SignedSpaceRoot, Space, SpaceNode};

/// Current [`OwnRoomSnapshot::v`].
pub const OWN_ROOM_SNAPSHOT_VERSION: u32 = 1;
/// Rooms one snapshot may carry; more are left out, not refused.
pub const MAX_SNAPSHOT_ROOMS: usize = 512;
/// Spaces one snapshot may carry.
pub const MAX_SNAPSHOT_SPACES: usize = 64;
/// Nodes one Space in a snapshot may hold.
pub const MAX_SNAPSHOT_SPACE_NODES: usize = 1024;

/// What one device tells its siblings about its rooms.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OwnRoomSnapshot {
    pub v: u32,
    #[serde(default)]
    pub rooms: Vec<RoomEntry>,
    #[serde(default)]
    pub spaces: Vec<Space>,
}

impl OwnRoomSnapshot {
    /// Whether a receiver should look at this snapshot at all.
    pub fn within_limits(&self) -> bool {
        self.v >= OWN_ROOM_SNAPSHOT_VERSION
            && self.rooms.len() <= MAX_SNAPSHOT_ROOMS
            && self.spaces.len() <= MAX_SNAPSHOT_SPACES
            && self
                .spaces
                .iter()
                .all(|space| space.nodes.len() <= MAX_SNAPSHOT_SPACE_NODES)
    }
}

/// The outcome of [`RoomStore::merge_own_device_snapshot`].
#[derive(Debug, Default)]
pub struct OwnRoomMerge {
    /// The store changed, so the room list should be redrawn.
    pub changed: bool,
    /// Roots re-signed by this merge, as `(supernode_id, root)`, to announce.
    pub roots: Vec<(String, SignedSpaceRoot)>,
    /// A Space here now differs from the sender's copy, so the sender should
    /// be sent ours. A merge that ends equal to the sender's copy leaves this
    /// unset, which is what stops two devices trading snapshots forever.
    pub sibling_behind: bool,
}

enum SpaceMerge {
    Unchanged,
    /// Take the sender's copy as it is. Its root was already signed and
    /// announced by the sender; the same nodes at the same epoch give the same
    /// root here.
    Adopt(Space),
    /// Neither copy holds the union, so it needs a new epoch and signature.
    Resign(Space),
}

fn same_identity(a: &str, b: &str) -> bool {
    a.trim_end_matches('=') == b.trim_end_matches('=')
}

/// A Space we can merge: ours, well formed, rooted where its id says.
fn space_is_owned_by(space: &Space, owner_pub: &str) -> bool {
    if !same_identity(&space.owner_pub, owner_pub) {
        return false;
    }
    let Some(server) = space.nodes.iter().find(|n| n.node_id == space.space_id) else {
        return false;
    };
    if server.kind != "server"
        || !server.parent_id.is_empty()
        || space.space_id != derive_node_id("", &space.owner_pub, &server.name)
    {
        return false;
    }
    let mut ids = std::collections::HashSet::new();
    space.nodes.iter().all(|node| {
        !node.node_id.is_empty()
            && ids.insert(node.node_id.as_str())
            && same_identity(&node.owner_pub, owner_pub)
            && (node.node_id == space.space_id || !node.parent_id.is_empty())
    })
}

fn sorted_nodes(space: &Space) -> Vec<SpaceNode> {
    let mut nodes = space.nodes.clone();
    nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    nodes
}

/// Union two copies of one Space. Where both hold a node with different
/// contents, the copy with the higher epoch wins, and on a tie the greater leaf
/// hash, so both devices pick the same node.
fn merge_space(local: Option<&Space>, incoming: &Space) -> SpaceMerge {
    let Some(local) = local else {
        return SpaceMerge::Adopt(incoming.clone());
    };
    let mut merged: BTreeMap<&str, &SpaceNode> = local
        .nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node))
        .collect();
    for node in &incoming.nodes {
        match merged.get(node.node_id.as_str()) {
            None => {
                merged.insert(node.node_id.as_str(), node);
            }
            Some(mine) if *mine != node => {
                let theirs_wins = incoming.epoch > local.epoch
                    || (incoming.epoch == local.epoch && node.leaf_hash() > mine.leaf_hash());
                if theirs_wins {
                    merged.insert(node.node_id.as_str(), node);
                }
            }
            Some(_) => {}
        }
    }
    let nodes: Vec<SpaceNode> = merged.into_values().cloned().collect();
    let local_same = sorted_nodes(local) == nodes;
    let incoming_same = sorted_nodes(incoming) == nodes;
    if incoming_same && incoming.epoch >= local.epoch {
        if local_same && incoming.epoch == local.epoch {
            SpaceMerge::Unchanged
        } else {
            SpaceMerge::Adopt(incoming.clone())
        }
    } else if local_same && local.epoch >= incoming.epoch {
        SpaceMerge::Unchanged
    } else {
        // Past both epochs: the sender has announced its own, and a root at an
        // epoch the supernode already holds is not accepted.
        SpaceMerge::Resign(Space {
            space_id: local.space_id.clone(),
            owner_pub: local.owner_pub.clone(),
            epoch: local.epoch.max(incoming.epoch) + 1,
            nodes,
        })
    }
}

/// Fill `existing`'s empty fields from `incoming`. Never overwrites.
fn fill_missing(existing: &mut RoomEntry, incoming: &RoomEntry) -> bool {
    let mut changed = false;
    let mut fill = |field: &mut String, value: &String| {
        if field.is_empty() && !value.is_empty() {
            *field = value.clone();
            changed = true;
        }
    };
    fill(&mut existing.creator_id, &incoming.creator_id);
    fill(&mut existing.invite_token, &incoming.invite_token);
    fill(&mut existing.space_id, &incoming.space_id);
    fill(&mut existing.parent_id, &incoming.parent_id);
    fill(&mut existing.invite_policy, &incoming.invite_policy);
    fill(&mut existing.room_type, &incoming.room_type);
    // A name learned only as the room id is a placeholder, not a name.
    let placeholder = existing.room_name.is_empty() || existing.room_name == existing.room_id;
    if placeholder && !incoming.room_name.is_empty() && incoming.room_name != incoming.room_id {
        existing.room_name = incoming.room_name.clone();
        changed = true;
    }
    if incoming.is_creator && !existing.is_creator {
        existing.is_creator = true;
        changed = true;
    }
    changed
}

impl RoomStore {
    /// What this device knows about its rooms, for its sibling devices.
    ///
    /// Leaves out rooms hidden here and the built-in `default` room, and every
    /// Space not owned by `owner_pub`.
    pub fn own_device_snapshot(&self, owner_pub: &str) -> OwnRoomSnapshot {
        let mut rooms: Vec<RoomEntry> = self
            .rooms
            .values()
            .filter(|entry| {
                entry.room_id != "default"
                    && !self
                        .deleted_ids
                        .contains(&Self::entry_key(&entry.supernode_id, &entry.room_id))
            })
            .cloned()
            .collect();
        rooms.sort_by(|a, b| {
            a.supernode_id
                .cmp(&b.supernode_id)
                .then_with(|| a.room_id.cmp(&b.room_id))
        });
        rooms.truncate(MAX_SNAPSHOT_ROOMS);
        let mut spaces: Vec<Space> = self
            .spaces
            .values()
            .filter(|space| {
                same_identity(&space.owner_pub, owner_pub)
                    && space.nodes.len() <= MAX_SNAPSHOT_SPACE_NODES
            })
            .cloned()
            .collect();
        spaces.sort_by(|a, b| a.space_id.cmp(&b.space_id));
        spaces.truncate(MAX_SNAPSHOT_SPACES);
        OwnRoomSnapshot {
            v: OWN_ROOM_SNAPSHOT_VERSION,
            rooms,
            spaces,
        }
    }

    /// Merge a sibling device's snapshot, persisting once if anything changed.
    ///
    /// `resolve_host` maps a supernode id to the spelling this store files
    /// rooms under, or `None` for a supernode this device does not know, whose
    /// rooms it could not show. `same_host` says whether two ids name one host,
    /// such as two members of a cluster, so a room already filed under a
    /// sibling member is not filed twice. `sign` signs re-built Space roots.
    pub fn merge_own_device_snapshot(
        &mut self,
        snapshot: &OwnRoomSnapshot,
        owner_pub: &str,
        resolve_host: impl Fn(&str) -> Option<String>,
        same_host: impl Fn(&str, &str) -> bool,
        issued_at: u64,
        sign: impl Fn(&[u8]) -> Vec<u8>,
    ) -> Result<OwnRoomMerge> {
        let mut out = OwnRoomMerge::default();
        if !snapshot.within_limits() {
            debug!("RoomStore: ignoring an own-device room snapshot outside its limits");
            return Ok(out);
        }

        for incoming in &snapshot.spaces {
            if !space_is_owned_by(incoming, owner_pub) {
                debug!("RoomStore: skipping a synced Space that is not ours or malformed");
                continue;
            }
            let Some(host) = incoming
                .nodes
                .iter()
                .find(|node| node.node_id == incoming.space_id)
                .map(|server| server.name.clone())
            else {
                continue;
            };
            if resolve_host(&host).is_none() {
                continue;
            }
            let merged = match merge_space(self.spaces.get(&incoming.space_id), incoming) {
                SpaceMerge::Unchanged => None,
                SpaceMerge::Adopt(space) => Some(space),
                SpaceMerge::Resign(space) => {
                    out.roots
                        .push((host.clone(), space.signed_root(issued_at, &sign)));
                    Some(space)
                }
            };
            if let Some(space) = merged {
                info!(
                    "RoomStore: Space {} now at epoch {} with {} node(s) from a sibling device",
                    &space.space_id[..space.space_id.len().min(12)],
                    space.epoch,
                    space.nodes.len()
                );
                self.spaces.insert(space.space_id.clone(), space);
                out.changed = true;
            }
            if let Some(ours) = self.spaces.get(&incoming.space_id) {
                if ours.epoch != incoming.epoch || sorted_nodes(ours) != sorted_nodes(incoming) {
                    out.sibling_behind = true;
                }
            }
        }

        for incoming in &snapshot.rooms {
            out.changed |= self.merge_own_device_room(incoming, &resolve_host, &same_host);
        }
        out.changed |= self.stamp_linkage_from_own_spaces(owner_pub, &same_host);

        if out.changed {
            self.save()?;
        }
        Ok(out)
    }

    fn merge_own_device_room(
        &mut self,
        incoming: &RoomEntry,
        resolve_host: &impl Fn(&str) -> Option<String>,
        same_host: &impl Fn(&str, &str) -> bool,
    ) -> bool {
        if incoming.room_id.is_empty() || incoming.room_id == "default" {
            return false;
        }
        let Some(host) = resolve_host(&incoming.supernode_id) else {
            return false;
        };
        let key = Self::entry_key(&host, &incoming.room_id);
        let existing = if self.rooms.contains_key(&key) {
            Some(key.clone())
        } else {
            self.rooms
                .iter()
                .find(|(_, entry)| {
                    entry.room_id == incoming.room_id && same_host(&entry.supernode_id, &host)
                })
                .map(|(existing_key, _)| existing_key.clone())
        };
        if let Some(existing) = existing {
            return self
                .rooms
                .get_mut(&existing)
                .is_some_and(|entry| fill_missing(entry, incoming));
        }
        // Hidden here, under this host or a sibling member of it.
        let hidden = self.deleted_ids.iter().any(|tombstone| {
            tombstone.split_once(':').is_some_and(|(sn, room)| {
                room == incoming.room_id && (sn == host || same_host(sn, &host))
            })
        });
        if hidden {
            return false;
        }
        info!(
            "RoomStore: adding room '{}' ({}) from a sibling device",
            incoming.room_name,
            &incoming.room_id[..incoming.room_id.len().min(12)]
        );
        let mut entry = incoming.clone();
        entry.supernode_id = host;
        self.rooms.insert(key, entry);
        true
    }

    /// Point each of our rooms at its place in our Space tree.
    ///
    /// For a room we created the Space is the record of where it nests; after
    /// a merge the tree may have moved on from what the entry was stamped with.
    fn stamp_linkage_from_own_spaces(
        &mut self,
        owner_pub: &str,
        same_host: &impl Fn(&str, &str) -> bool,
    ) -> bool {
        let mut changed = false;
        for space in self.spaces.values() {
            if !same_identity(&space.owner_pub, owner_pub) {
                continue;
            }
            let Some(host) = space
                .nodes
                .iter()
                .find(|node| node.node_id == space.space_id)
                .map(|server| server.name.as_str())
            else {
                continue;
            };
            for node in space.nodes.iter().filter(|node| node.kind == "room") {
                for entry in self.rooms.values_mut() {
                    if entry.room_id != node.node_id
                        || !(entry.supernode_id == host || same_host(&entry.supernode_id, host))
                    {
                        continue;
                    }
                    if entry.parent_id != node.parent_id || entry.space_id != space.space_id {
                        entry.parent_id = node.parent_id.clone();
                        entry.space_id = space.space_id.clone();
                        changed = true;
                    }
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;
    use tempfile::tempdir;

    const HOST: &str = "node-a";

    fn store(identity: &Identity, dir: &tempfile::TempDir, name: &str) -> RoomStore {
        RoomStore::open(identity, Some(&dir.path().join(name))).unwrap()
    }

    fn known(id: &str) -> Option<String> {
        (id == HOST || id == "node-b").then(|| id.to_owned())
    }

    fn cluster(a: &str, b: &str) -> bool {
        let members = ["node-a", "node-b"];
        members.contains(&a) && members.contains(&b)
    }

    fn separate(a: &str, b: &str) -> bool {
        a == b
    }

    /// Create a room the way a client does: store it, then adopt it.
    fn create(store: &mut RoomStore, identity: &Identity, room: &str, parent: &str) {
        let owner = identity.public_id();
        store
            .upsert(
                RoomEntry::new(room, room.to_uppercase())
                    .with_supernode(HOST)
                    .with_creator(&owner, true)
                    .with_invite_token(format!("tok-{room}")),
            )
            .unwrap();
        store
            .adopt_room_into_space(
                &owner,
                HOST,
                room,
                &room.to_uppercase(),
                "public",
                parent,
                1,
                |b| identity.sign(b),
            )
            .unwrap();
    }

    fn merge(into: &mut RoomStore, from: &RoomStore, identity: &Identity) -> OwnRoomMerge {
        let owner = identity.public_id();
        into.merge_own_device_snapshot(
            &from.own_device_snapshot(&owner),
            &owner,
            known,
            cluster,
            2,
            |b| identity.sign(b),
        )
        .unwrap()
    }

    fn space_of(store: &RoomStore, identity: &Identity) -> Space {
        store
            .get_space(&RoomStore::space_id_for(&identity.public_id(), HOST))
            .unwrap()
    }

    #[test]
    fn a_room_made_on_one_device_appears_on_the_other_with_its_parent() {
        let dir = tempdir().unwrap();
        let identity = Identity::generate();
        let mut phone = store(&identity, &dir, "phone.dat");
        let mut desktop = store(&identity, &dir, "desktop.dat");
        create(&mut phone, &identity, "place", "");
        create(&mut phone, &identity, "general", "place");

        let result = merge(&mut desktop, &phone, &identity);
        assert!(result.changed);
        assert!(result.roots.is_empty(), "an adopted tree needs no new root");
        assert!(!result.sibling_behind);
        let general = desktop.get(HOST, "general").unwrap();
        assert_eq!(general.parent_id, "place");
        assert!(general.is_creator);
        assert_eq!(general.invite_token, "tok-general");
        assert_eq!(
            space_of(&desktop, &identity).epoch,
            space_of(&phone, &identity).epoch
        );

        // The merge persisted.
        let reopened = store(&identity, &dir, "desktop.dat");
        assert_eq!(reopened.get(HOST, "general").unwrap().parent_id, "place");

        // Nothing new the second time.
        let again = merge(&mut desktop, &phone, &identity);
        assert!(!again.changed && again.roots.is_empty() && !again.sibling_behind);
    }

    #[test]
    fn diverged_trees_are_resigned_past_both_epochs_and_then_converge() {
        let dir = tempdir().unwrap();
        let identity = Identity::generate();
        let mut phone = store(&identity, &dir, "phone.dat");
        let mut desktop = store(&identity, &dir, "desktop.dat");
        create(&mut phone, &identity, "phone-room", "");
        create(&mut desktop, &identity, "desk-room", "");
        create(&mut desktop, &identity, "desk-sub", "desk-room");
        let phone_epoch = space_of(&phone, &identity).epoch;
        let desk_epoch = space_of(&desktop, &identity).epoch;

        let first = merge(&mut desktop, &phone, &identity);
        assert_eq!(first.roots.len(), 1);
        assert_eq!(first.roots[0].0, HOST);
        assert!(first.roots[0].1.verify());
        assert!(first.sibling_behind);
        let merged = space_of(&desktop, &identity);
        assert_eq!(merged.epoch, phone_epoch.max(desk_epoch) + 1);
        assert_eq!(first.roots[0].1.epoch, merged.epoch);
        assert!(desktop.get(HOST, "phone-room").is_some());

        // The phone takes the desktop's reply as it is.
        let reply = merge(&mut phone, &desktop, &identity);
        assert!(reply.roots.is_empty());
        assert!(!reply.sibling_behind);
        assert_eq!(space_of(&phone, &identity).root_hash(), merged.root_hash());
        assert_eq!(space_of(&phone, &identity).epoch, merged.epoch);
        assert_eq!(phone.get(HOST, "desk-sub").unwrap().parent_id, "desk-room");
    }

    #[test]
    fn a_conflicting_node_resolves_the_same_way_on_both_devices() {
        let dir = tempdir().unwrap();
        let identity = Identity::generate();
        let mut phone = store(&identity, &dir, "phone.dat");
        create(&mut phone, &identity, "a", "");
        create(&mut phone, &identity, "b", "");
        create(&mut phone, &identity, "c", "a");
        let mut desktop = store(&identity, &dir, "desktop.dat");
        merge(&mut desktop, &phone, &identity);
        // Each moves "c" somewhere else, at the same epoch.
        create(&mut phone, &identity, "c", "b");
        create(&mut desktop, &identity, "c", "");
        let on_desktop = {
            let mut copy = store(&identity, &dir, "desktop.dat");
            merge(&mut copy, &phone, &identity);
            space_of(&copy, &identity)
        };
        let on_phone = {
            let mut copy = store(&identity, &dir, "phone.dat");
            merge(&mut copy, &desktop, &identity);
            space_of(&copy, &identity)
        };
        assert_eq!(on_desktop.root_hash(), on_phone.root_hash());
        assert_eq!(on_desktop.epoch, on_phone.epoch);
    }

    #[test]
    fn merging_fills_gaps_but_never_overwrites() {
        let dir = tempdir().unwrap();
        let identity = Identity::generate();
        let owner = identity.public_id();
        let mut desktop = store(&identity, &dir, "desktop.dat");
        desktop
            .upsert(
                RoomEntry::new("r1", "r1")
                    .with_supernode(HOST)
                    .with_type("private"),
            )
            .unwrap();
        desktop
            .upsert(
                RoomEntry::new("r2", "Mine")
                    .with_supernode(HOST)
                    .with_invite_token("keep"),
            )
            .unwrap();
        let snapshot = OwnRoomSnapshot {
            v: 1,
            rooms: vec![
                RoomEntry::new("r1", "Lounge")
                    .with_supernode(HOST)
                    .with_type("public")
                    .with_creator(&owner, true)
                    .with_invite_token("t1"),
                RoomEntry::new("r2", "Theirs")
                    .with_supernode(HOST)
                    .with_invite_token("other"),
            ],
            spaces: vec![],
        };
        let result = desktop
            .merge_own_device_snapshot(&snapshot, &owner, known, separate, 2, |b| identity.sign(b))
            .unwrap();
        assert!(result.changed);
        let r1 = desktop.get(HOST, "r1").unwrap();
        assert_eq!(r1.room_name, "Lounge", "a placeholder name is filled");
        assert_eq!(r1.room_type, "private", "the type is never overwritten");
        assert_eq!(r1.invite_token, "t1");
        assert!(r1.is_creator);
        let r2 = desktop.get(HOST, "r2").unwrap();
        assert_eq!(r2.room_name, "Mine");
        assert_eq!(r2.invite_token, "keep");
    }

    #[test]
    fn hidden_rooms_stay_local_in_both_directions() {
        let dir = tempdir().unwrap();
        let identity = Identity::generate();
        let mut phone = store(&identity, &dir, "phone.dat");
        let mut desktop = store(&identity, &dir, "desktop.dat");
        create(&mut phone, &identity, "shown", "");
        create(&mut phone, &identity, "hidden-there", "");
        phone.hide_from_sidebar(HOST, "hidden-there").unwrap();
        create(&mut phone, &identity, "hidden-here", "");
        desktop.hide_from_sidebar("node-b", "hidden-here").unwrap();

        let owner = identity.public_id();
        let snapshot = phone.own_device_snapshot(&owner);
        assert!(snapshot.rooms.iter().all(|r| r.room_id != "hidden-there"));
        merge(&mut desktop, &phone, &identity);
        assert!(desktop.get(HOST, "shown").is_some());
        assert!(desktop.get(HOST, "hidden-there").is_none());
        // Hidden against a sibling member of the same cluster.
        assert!(desktop.get(HOST, "hidden-here").is_none());
    }

    #[test]
    fn a_room_already_filed_under_a_cluster_sibling_is_not_filed_twice() {
        let dir = tempdir().unwrap();
        let identity = Identity::generate();
        let owner = identity.public_id();
        let mut desktop = store(&identity, &dir, "desktop.dat");
        desktop
            .upsert(RoomEntry::new("r", "Room").with_supernode("node-b"))
            .unwrap();
        let snapshot = OwnRoomSnapshot {
            v: 1,
            rooms: vec![RoomEntry::new("r", "Room")
                .with_supernode(HOST)
                .with_invite_token("t")],
            spaces: vec![],
        };
        desktop
            .merge_own_device_snapshot(&snapshot, &owner, known, cluster, 2, |b| identity.sign(b))
            .unwrap();
        assert!(desktop.get(HOST, "r").is_none());
        assert_eq!(desktop.get("node-b", "r").unwrap().invite_token, "t");
    }

    #[test]
    fn unknown_hosts_foreign_spaces_and_oversized_snapshots_are_ignored() {
        let dir = tempdir().unwrap();
        let identity = Identity::generate();
        let stranger = Identity::generate();
        let owner = identity.public_id();
        let mut desktop = store(&identity, &dir, "desktop.dat");

        let mut theirs = store(&stranger, &dir, "stranger.dat");
        create(&mut theirs, &stranger, "x", "");
        let mut foreign = theirs.own_device_snapshot(&stranger.public_id());
        foreign
            .rooms
            .push(RoomEntry::new("far", "Far").with_supernode("node-z"));
        let result = desktop
            .merge_own_device_snapshot(&foreign, &owner, known, separate, 2, |b| identity.sign(b))
            .unwrap();
        assert!(result.roots.is_empty());
        assert!(
            desktop.all_spaces().is_empty(),
            "a Space we do not own is not merged"
        );
        assert!(
            desktop.get("node-z", "far").is_none(),
            "an unknown host is skipped"
        );

        let big = OwnRoomSnapshot {
            v: 1,
            rooms: (0..=MAX_SNAPSHOT_ROOMS)
                .map(|i| RoomEntry::new(format!("r{i}"), "n").with_supernode(HOST))
                .collect(),
            spaces: vec![],
        };
        let result = desktop
            .merge_own_device_snapshot(&big, &owner, known, separate, 2, |b| identity.sign(b))
            .unwrap();
        assert!(!result.changed);
        assert!(desktop.get(HOST, "r0").is_none());
    }

    #[test]
    fn a_tampered_space_is_refused() {
        let dir = tempdir().unwrap();
        let identity = Identity::generate();
        let owner = identity.public_id();
        let mut phone = store(&identity, &dir, "phone.dat");
        create(&mut phone, &identity, "r", "");
        let mut snapshot = phone.own_device_snapshot(&owner);
        // A second root node: the tree no longer hangs from its Server node.
        snapshot.spaces[0].nodes[1].parent_id.clear();
        let mut desktop = store(&identity, &dir, "desktop.dat");
        desktop
            .merge_own_device_snapshot(&snapshot, &owner, known, separate, 2, |b| identity.sign(b))
            .unwrap();
        assert!(desktop.all_spaces().is_empty());
    }
}
