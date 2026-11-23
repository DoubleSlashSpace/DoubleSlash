use super::*;

#[test]
fn strangers_stay_untrusted_across_block_unblock_and_reload() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(PEER_STORE_FILE);
    let identity = Identity::generate();
    let stranger = Identity::generate().public_id();
    let mut store = PeerStore::open(&identity, Some(&path))?;
    store.set_identity_blocked(&stranger, "  Room\u{202e}\nmember  ", true)?;
    assert!(store.list_peers().is_empty());
    let mut reopened = PeerStore::open(&identity, Some(&path))?;
    assert!(reopened.is_blocked(stranger.trim_end_matches('=')));
    assert_eq!(reopened.blocked_people()[0].label, "Roommember");
    assert!(reopened.find_identity(&stranger).is_none());
    reopened.set_identity_blocked(stranger.trim_end_matches('='), "", false)?;
    let reopened = PeerStore::open(&identity, Some(&path))?;
    assert!(!reopened.is_blocked(&stranger));
    assert!(reopened.list_peers().is_empty());
    assert!(reopened.blocked_people().is_empty());
    Ok(())
}

#[test]
fn legacy_blocks_survive_contact_removal_and_invite_replacement() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(PEER_STORE_FILE);
    let identity = Identity::generate();
    let public_id = Identity::generate().public_id();
    let record = PeerRecord {
        peer_id: "hex".into(),
        identity_pub: public_id.clone(),
        blocked: true,
        handle: "Old peer".into(),
        ..Default::default()
    };
    let legacy = serde_json::to_vec(&serde_json::json!({ "version": 1, "peers": [record] }))?;
    let key = identity.derive_store_key(PEER_STORE_LABEL)?;
    std::fs::write(&path, encrypt_blob(&key, &legacy)?)?;
    let mut store = PeerStore::open(&identity, Some(&path))?;
    assert!(store.is_blocked("hex"));
    store.upsert_from_invite(PeerRecord {
        peer_id: "replacement".into(),
        identity_pub: public_id.clone(),
        ..Default::default()
    });
    assert!(store.get("replacement").is_some_and(|p| p.blocked));
    store.remove("replacement");
    store.save()?;
    let mut store = PeerStore::open(&identity, Some(&path))?;
    assert!(store.is_blocked(&public_id));
    assert!(store.list_peers().is_empty());
    store.set_identity_blocked(&public_id, "", false)?;
    assert!(store.list_peers().is_empty());
    Ok(())
}

#[test]
fn failed_saves_and_invalid_ids_never_claim_a_block() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let identity = Identity::generate();
    let path = dir.path().join("a-file");
    std::fs::write(&path, b"not a directory")?;
    let mut store = PeerStore::open(&identity, Some(&path.join("peers.dat")))?;
    let stranger = Identity::generate().public_id();
    assert!(store
        .set_identity_blocked(&stranger, "Member", true)
        .is_err());
    assert!(!store.is_blocked(&stranger));
    assert!(store.blocked_people().is_empty());
    assert!(store
        .set_identity_blocked("not-an-identity", "", true)
        .is_err());
    assert!(store.list_peers().is_empty());
    Ok(())
}
