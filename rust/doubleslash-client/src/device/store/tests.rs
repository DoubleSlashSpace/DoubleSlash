use super::*;
use crate::device::DeviceKey;

#[test]
fn backup_reads_never_initialize_missing_or_empty_stores() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("device-trust.sqlite");
    let owner = Identity::generate();
    assert!(DeviceTrustStore::read_existing(&owner, &path).is_err());
    assert!(!path.exists());
    std::fs::write(&path, []).unwrap();
    assert!(DeviceTrustStore::read_existing(&owner, &path).is_err());
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
}

#[test]
fn committed_revocation_survives_restart_and_rejects_old_proofs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("device-trust.sqlite");
    let owner = Identity::generate();
    let peer = Identity::generate();
    let phone = DeviceKey::generate();
    let initial = DeviceRegistry::create(&peer, phone.entry("Phone")).unwrap();
    let proof = phone.prove(&initial, &[1; 32], &[2; 32]).unwrap();
    let mut store = DeviceTrustStore::open(&owner, &path).unwrap();
    assert!(store
        .accept(&peer.public_key_bytes(), &initial.to_bytes().unwrap())
        .unwrap());
    assert!(store
        .verify_proof(
            &peer.public_key_bytes(),
            phone.id(),
            &[1; 32],
            &[2; 32],
            &proof
        )
        .unwrap());
    let mut entry = phone.entry("Phone");
    entry.revoked = true;
    let revoked = initial.update(&peer, entry).unwrap();
    assert!(store
        .accept(&peer.public_key_bytes(), &revoked.to_bytes().unwrap())
        .unwrap());
    drop(store);
    let mut reopened = DeviceTrustStore::open(&owner, &path).unwrap();
    assert!(!reopened
        .verify_proof(
            &peer.public_key_bytes(),
            phone.id(),
            &[1; 32],
            &[2; 32],
            &proof
        )
        .unwrap());
    assert!(reopened
        .accept(&peer.public_key_bytes(), &initial.to_bytes().unwrap())
        .is_err());
    assert!(!reopened
        .accept(&peer.public_key_bytes(), &revoked.to_bytes().unwrap())
        .unwrap());
}

#[test]
fn independent_handles_observe_latest_version_and_reject_forks() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("device-trust.sqlite");
    let owner = Identity::generate();
    let peer = Identity::generate();
    let phone = DeviceKey::generate();
    let initial = DeviceRegistry::create(&peer, phone.entry("Phone")).unwrap();
    let mut first = DeviceTrustStore::open(&owner, &path).unwrap();
    let mut stale = DeviceTrustStore::open(&owner, &path).unwrap();
    first
        .accept(&peer.public_key_bytes(), &initial.to_bytes().unwrap())
        .unwrap();
    let next = initial.update(&peer, phone.entry("Renamed")).unwrap();
    first
        .accept(&peer.public_key_bytes(), &next.to_bytes().unwrap())
        .unwrap();
    let fork = initial
        .update(&peer, phone.entry("Conflicting name"))
        .unwrap();
    assert!(stale
        .accept(&peer.public_key_bytes(), &fork.to_bytes().unwrap())
        .is_err());
    assert!(stale
        .accept(&peer.public_key_bytes(), &initial.to_bytes().unwrap())
        .is_err());
    assert_eq!(
        stale
            .get(&peer.public_key_bytes())
            .unwrap()
            .unwrap()
            .entries()[0]
            .name,
        "Renamed"
    );
}

#[test]
fn competing_writers_commit_only_one_successor() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("device-trust.sqlite");
    let owner = Identity::generate();
    let peer = Identity::generate();
    let phone = DeviceKey::generate();
    let initial = DeviceRegistry::create(&peer, phone.entry("Phone")).unwrap();
    let mut first = DeviceTrustStore::open(&owner, &path).unwrap();
    let mut second = DeviceTrustStore::open(&owner, &path).unwrap();
    first
        .accept(&peer.public_key_bytes(), &initial.to_bytes().unwrap())
        .unwrap();
    let left = initial
        .update(&peer, phone.entry("Left"))
        .unwrap()
        .to_bytes()
        .unwrap();
    let right = initial
        .update(&peer, phone.entry("Right"))
        .unwrap()
        .to_bytes()
        .unwrap();
    let barrier = std::sync::Barrier::new(2);
    let accepted = std::thread::scope(|scope| {
        let left = scope.spawn(|| {
            barrier.wait();
            first.accept(&peer.public_key_bytes(), &left).is_ok()
        });
        let right = scope.spawn(|| {
            barrier.wait();
            second.accept(&peer.public_key_bytes(), &right).is_ok()
        });
        usize::from(left.join().unwrap()) + usize::from(right.join().unwrap())
    });
    assert_eq!(accepted, 1);
}

#[test]
fn wrong_key_and_tampering_fail_closed_without_overwriting() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("device-trust.sqlite");
    let owner = Identity::generate();
    let peer = Identity::generate();
    let registry =
        DeviceRegistry::create(&peer, DeviceKey::generate().entry("Private phone name")).unwrap();
    let mut store = DeviceTrustStore::open(&owner, &path).unwrap();
    assert!(DeviceTrustStore::open(&Identity::generate(), &path).is_err());
    store
        .accept(&peer.public_key_bytes(), &registry.to_bytes().unwrap())
        .unwrap();
    let disk = std::fs::read(&path).unwrap();
    assert!(!disk
        .windows(b"Private phone name".len())
        .any(|part| part == b"Private phone name"));
    assert!(!disk.windows(32).any(|part| part == peer.public_key_bytes()));
    let lookup = store.lookup(&peer.public_key_bytes()).unwrap();
    store
        .connection
        .execute(
            "UPDATE registries SET envelope=zeroblob(?1) WHERE lookup=?2",
            params![MAX_ENVELOPE + 1, lookup.as_slice()],
        )
        .unwrap();
    assert!(store.get(&peer.public_key_bytes()).is_err());
    assert!(store
        .accept(&peer.public_key_bytes(), &registry.to_bytes().unwrap())
        .is_err());
    store
        .connection
        .execute("UPDATE registries SET envelope=X'000102'", [])
        .unwrap();
    assert!(store.get(&peer.public_key_bytes()).is_err());
}

#[test]
fn failed_write_does_not_publish_new_authorization() {
    let directory = tempfile::tempdir().unwrap();
    let owner = Identity::generate();
    let phone = DeviceKey::generate();
    let registry = DeviceRegistry::create(&owner, phone.entry("Phone")).unwrap();
    let mut store =
        DeviceTrustStore::open(&owner, &directory.path().join("device-trust.sqlite")).unwrap();
    store.connection.execute_batch("CREATE TRIGGER reject_write BEFORE INSERT ON registries BEGIN SELECT RAISE(ABORT, 'simulated write failure'); END;").unwrap();
    assert!(store
        .accept(&owner.public_key_bytes(), &registry.to_bytes().unwrap())
        .is_err());
    assert!(store.get(&owner.public_key_bytes()).unwrap().is_none());
    store
        .connection
        .execute_batch("DROP TRIGGER reject_write")
        .unwrap();
    assert!(store
        .accept(&owner.public_key_bytes(), &registry.to_bytes().unwrap())
        .unwrap());
}

#[test]
fn database_versions_and_identity_binding_are_enforced() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("device-trust.sqlite");
    let owner = Identity::generate();
    let other = Identity::generate();
    let mut store = DeviceTrustStore::open(&owner, &path).unwrap();
    let registry = DeviceRegistry::create(&owner, DeviceKey::generate().entry("Phone")).unwrap();
    assert!(store
        .accept(&other.public_key_bytes(), &registry.to_bytes().unwrap())
        .is_err());
    assert!(store
        .accept(&owner.public_key_bytes(), &vec![0; MAX_REGISTRY_BYTES + 1])
        .is_err());
    store
        .accept(&owner.public_key_bytes(), &registry.to_bytes().unwrap())
        .unwrap();
    let other_lookup = store.lookup(&other.public_key_bytes()).unwrap();
    store
        .connection
        .execute("UPDATE registries SET lookup=?1", [other_lookup.as_slice()])
        .unwrap();
    assert!(store.get(&other.public_key_bytes()).is_err());
    store
        .connection
        .pragma_update(None, "user_version", 2)
        .unwrap();
    drop(store);
    assert!(DeviceTrustStore::open(&owner, &path).is_err());
}
