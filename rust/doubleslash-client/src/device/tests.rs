use super::*;

#[test]
fn existing_stores_can_open_with_subkeys_without_identity_seed() -> Result<()> {
    use crate::chat_store::{ChatMessage, ChatStore, MessageKind, MessageStatus, CHAT_STORE_LABEL};
    use crate::peer_store::{PeerRecord, PeerStore, PEER_STORE_LABEL};
    use crate::room_store::{RoomEntry, RoomStore, ROOM_STORE_LABEL};

    let directory = tempfile::tempdir()?;
    let identity = Identity::generate();
    let peers_key = Zeroizing::new(identity.derive_store_key(PEER_STORE_LABEL)?);
    let rooms_key = Zeroizing::new(identity.derive_store_key(ROOM_STORE_LABEL)?);
    let chat_key = Zeroizing::new(identity.derive_store_key(CHAT_STORE_LABEL)?);
    let peers_path = directory.path().join("peers.dat");
    let rooms_path = directory.path().join("my_rooms.dat");
    let chat_path = directory.path().join("chat_history.db");
    let mut peers = PeerStore::open(&identity, Some(&peers_path))?;
    peers.upsert(PeerRecord {
        peer_id: "friend".into(),
        blocked: true,
        ..Default::default()
    });
    peers.save()?;
    let mut rooms = RoomStore::open(&identity, Some(&rooms_path))?;
    rooms.add(RoomEntry::new("room", "Room").with_supernode("host"))?;
    rooms.hide_from_sidebar("host", "room")?;
    let chat = ChatStore::open(&identity, Some(&chat_path))?;
    chat.insert(&ChatMessage {
        id: "message".into(),
        peer_id: "friend".into(),
        sender: "friend".into(),
        recipient: identity.public_id(),
        body: "encrypted history".into(),
        timestamp: 1.0,
        is_self: false,
        status: MessageStatus::Delivered,
        kind: MessageKind::Text,
        attachment_name: String::new(),
        attachment_path: String::new(),
        size_str: String::new(),
        status_note: String::new(),
        sender_handle: String::new(),
    })?;
    drop((identity, peers, rooms, chat));

    let peers = PeerStore::open_with_key(&peers_key, &peers_path)?;
    assert!(peers.get("friend").unwrap().blocked);
    let rooms = RoomStore::open_with_key(&rooms_key, &rooms_path)?;
    assert!(rooms.is_hidden_from_sidebar("host", "room"));
    let chat = ChatStore::open_with_key(&chat_key, &chat_path)?;
    assert_eq!(
        chat.get_by_id("message")?.unwrap().body,
        "encrypted history"
    );
    // Domain-separated material does not decrypt a different store.
    assert!(RoomStore::open_with_key(&peers_key, &rooms_path).is_err());
    Ok(())
}

#[test]
fn one_identity_authorizes_two_independent_endpoints() {
    let identity = Identity::generate();
    let desktop = DeviceKey::generate();
    let phone = DeviceKey::generate();
    let registry = DeviceRegistry::create(&identity, desktop.entry("Desktop"))
        .unwrap()
        .update(&identity, phone.entry("Phone"))
        .unwrap();
    assert_ne!(desktop.id(), phone.id());
    assert_ne!(desktop.id().0, identity.public_key_bytes());
    let accepted = DeviceRegistry::verify(
        &registry.to_bytes().unwrap(),
        &identity.public_key_bytes(),
        None,
    )
    .unwrap();
    for device in [&desktop, &phone] {
        let proof = device.prove(&accepted, &[1; 32], &[2; 32]).unwrap();
        assert!(accepted.verify_proof(device.id(), &[1; 32], &[2; 32], &proof));
        assert!(!accepted.verify_proof(device.id(), &[3; 32], &[2; 32], &proof));
        assert!(!accepted.verify_proof(device.id(), &[1; 32], &[4; 32], &proof));
        assert!(!Identity::verify_with_public_key(
            &identity.public_key_bytes(),
            &proof,
            b"anything"
        ));
    }
}

#[test]
fn unrelated_identity_and_modified_documents_are_rejected() {
    let identity = Identity::generate();
    let device = DeviceKey::generate();
    let registry = DeviceRegistry::create(&identity, device.entry("Phone")).unwrap();
    let bytes = registry.to_bytes().unwrap();
    assert!(
        DeviceRegistry::verify(&bytes, &Identity::generate().public_key_bytes(), None).is_err()
    );
    let mut signed: SignedRegistry = serde_json::from_slice(&bytes).unwrap();
    signed.body.devices[0].name = "Attacker's name".into();
    assert!(DeviceRegistry::verify(
        &serde_json::to_vec(&signed).unwrap(),
        &identity.public_key_bytes(),
        None
    )
    .is_err());
    signed.body.devices[0] = DeviceKey::generate().entry("Phone");
    assert!(DeviceRegistry::verify(
        &serde_json::to_vec(&signed).unwrap(),
        &identity.public_key_bytes(),
        None
    )
    .is_err());
    assert!(registry
        .update(&Identity::generate(), device.entry("Other"))
        .is_err());
}

#[test]
fn revoked_device_cannot_prove_and_rollback_cannot_restore_access() {
    let identity = Identity::generate();
    let phone = DeviceKey::generate();
    let initial = DeviceRegistry::create(&identity, phone.entry("Phone")).unwrap();
    let old_proof = phone.prove(&initial, &[1; 32], &[2; 32]).unwrap();
    let mut revoked = phone.entry("Phone");
    revoked.revoked = true;
    let latest = initial.update(&identity, revoked).unwrap();
    assert!(!latest.verify_proof(phone.id(), &[1; 32], &[2; 32], &old_proof));
    assert!(phone.prove(&latest, &[1; 32], &[2; 32]).is_err());
    assert!(latest
        .update(&identity, phone.entry("Phone again"))
        .is_err());

    let mut store = DeviceRegistryStore::default();
    assert!(store
        .accept(&identity.public_key_bytes(), &initial.to_bytes().unwrap())
        .unwrap());
    assert!(store
        .accept(&identity.public_key_bytes(), &latest.to_bytes().unwrap())
        .unwrap());
    assert!(!store
        .accept(&identity.public_key_bytes(), &latest.to_bytes().unwrap())
        .unwrap());
    assert!(store
        .accept(&identity.public_key_bytes(), &initial.to_bytes().unwrap())
        .is_err());

    let mut after_restart = DeviceRegistryStore::default();
    for (id, bytes) in store.snapshots().unwrap() {
        after_restart.accept(&id, &bytes).unwrap();
    }
    assert!(!after_restart
        .get(&identity.public_key_bytes())
        .unwrap()
        .authorizes(phone.id()));
    assert!(after_restart
        .accept(&identity.public_key_bytes(), &initial.to_bytes().unwrap())
        .is_err());
}

#[test]
fn forks_and_tombstone_removal_fail_closed() {
    let identity = Identity::generate();
    let desktop = DeviceKey::generate();
    let phone = DeviceKey::generate();
    let initial = DeviceRegistry::create(&identity, desktop.entry("Desktop")).unwrap();
    let branch_a = initial.update(&identity, phone.entry("Phone A")).unwrap();
    let branch_b = initial.update(&identity, phone.entry("Phone B")).unwrap();
    assert!(DeviceRegistry::verify(
        &branch_b.to_bytes().unwrap(),
        &identity.public_key_bytes(),
        Some(&branch_a)
    )
    .is_err());
    let mut body = branch_a.signed.body.clone();
    body.version += 1;
    body.devices.retain(|d| d.id != desktop.id());
    let removed = DeviceRegistry::sign(&identity, body).unwrap();
    assert!(DeviceRegistry::verify(
        &removed.to_bytes().unwrap(),
        &identity.public_key_bytes(),
        Some(&branch_a)
    )
    .is_err());
}

#[test]
fn proofs_bind_registry_identity_version_and_device() {
    let owner_a = Identity::generate();
    let owner_b = Identity::generate();
    let device = DeviceKey::generate();
    let a = DeviceRegistry::create(&owner_a, device.entry("Desktop")).unwrap();
    let b = DeviceRegistry::create(&owner_b, device.entry("Desktop")).unwrap();
    let signature = device.prove(&a, &[1; 32], &[2; 32]).unwrap();
    assert!(!b.verify_proof(device.id(), &[1; 32], &[2; 32], &signature));
    let renamed = a.update(&owner_a, device.entry("New name")).unwrap();
    assert!(!renamed.verify_proof(device.id(), &[1; 32], &[2; 32], &signature));
    assert!(!a.verify_proof(DeviceKey::generate().id(), &[1; 32], &[2; 32], &signature));
    assert!(!a.verify_proof(device.id(), &[1; 32], &[2; 32], &[0; 63]));
}

#[test]
fn invalid_keys_names_duplicates_and_limits_are_rejected() {
    let identity = Identity::generate();
    let device = DeviceKey::generate();
    for name in ["", "  ", "Desktop\nforged log"] {
        assert!(DeviceRegistry::create(&identity, device.entry(name)).is_err());
    }
    assert!(DeviceRegistry::create(&identity, device.entry(&"a".repeat(129))).is_err());
    let mut entry = device.entry("Desktop");
    entry.id = DeviceId(identity.public_key_bytes());
    assert!(DeviceRegistry::create(&identity, entry.clone()).is_err());
    entry.id = DeviceId([0; 32]);
    assert!(DeviceRegistry::create(&identity, entry).is_err());
    let registry = DeviceRegistry::create(&identity, device.entry("Desktop")).unwrap();
    let mut body = registry.signed.body.clone();
    body.devices.push(device.entry("Duplicate"));
    assert!(DeviceRegistry::sign(&identity, body.clone()).is_err());
    body.devices = (0..=MAX_DEVICES)
        .map(|_| DeviceKey::generate().entry("Device"))
        .collect();
    assert!(DeviceRegistry::sign(&identity, body).is_err());
    assert!(DeviceRegistry::verify(
        &vec![0; MAX_REGISTRY_BYTES + 1],
        &identity.public_key_bytes(),
        None
    )
    .is_err());
}

#[test]
fn local_key_survives_restart_and_separate_profiles_get_distinct_keys() {
    let identity = Identity::generate();
    let desktop = tempfile::tempdir().unwrap();
    let phone = tempfile::tempdir().unwrap();
    let first = DeviceKey::load_or_create(&identity, desktop.path()).unwrap();
    let after_restart = DeviceKey::load_or_create(&identity, desktop.path()).unwrap();
    let restored = DeviceKey::load_or_create(&identity, phone.path()).unwrap();
    assert_eq!(first.id(), after_restart.id());
    assert_ne!(first.id(), restored.id());
    let bytes = std::fs::read(desktop.path().join(KEY_FILE)).unwrap();
    assert!(!bytes.windows(32).any(|w| w == first.signing.to_bytes()));
    assert!(DeviceKey::load_or_create(&Identity::generate(), desktop.path()).is_err());
    std::fs::write(desktop.path().join(KEY_FILE), b"corrupt").unwrap();
    assert!(DeviceKey::load_or_create(&identity, desktop.path()).is_err());
    assert_eq!(
        std::fs::read(desktop.path().join(KEY_FILE)).unwrap(),
        b"corrupt"
    );
}

#[test]
fn concurrent_first_starts_publish_one_local_key() {
    let identity = std::sync::Arc::new(Identity::generate());
    let directory = tempfile::tempdir().unwrap();
    let barrier = std::sync::Barrier::new(4);
    let ids = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    DeviceKey::load_or_create(&identity, directory.path())
                        .unwrap()
                        .id()
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(ids.iter().all(|id| *id == ids[0]));
}

#[test]
fn registry_encoding_and_digest_are_stable_across_json_layout() {
    let identity = Identity::generate();
    let registry = DeviceRegistry::create(&identity, DeviceKey::generate().entry("Phone")).unwrap();
    let pretty = serde_json::to_vec_pretty(&registry.signed).unwrap();
    let verified =
        DeviceRegistry::verify(&pretty, &identity.public_key_bytes(), Some(&registry)).unwrap();
    assert_eq!(
        registry_digest(&registry).unwrap(),
        registry_digest(&verified).unwrap()
    );
}
