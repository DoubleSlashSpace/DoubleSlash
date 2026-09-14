use super::*;
use base64::Engine;
use ed25519_dalek::{pkcs8::EncodePrivateKey, SigningKey};

fn client(root: &SigningKey, claimed_root: &[u8; 32], device: DeviceId) -> Endpoint {
    let mut params = rcgen::CertificateParams::default();
    params.distinguished_name.push(
        rcgen::DnType::CommonName,
        format!(
            "{}.{}",
            hex::encode(claimed_root),
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(device.0)
        ),
    );
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        root.to_pkcs8_der().unwrap().as_bytes().to_vec(),
    ));
    let pair = rcgen::KeyPair::from_der_and_sign_algo(&key, &rcgen::PKCS_ED25519).unwrap();
    let cert = params.self_signed(&pair).unwrap();
    let mut crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert))
        .with_client_auth_cert(vec![cert.der().clone()], key)
        .unwrap();
    crypto.alpn_protocols = vec![b"doubleslash/1".to_vec()];
    let config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).unwrap(),
    ));
    let mut endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(config);
    endpoint
}

async fn connect(endpoint: &Endpoint, port: u16) -> (quinn::Connection, u8) {
    let connection = tokio::time::timeout(
        Duration::from_secs(3),
        endpoint
            .connect(format!("127.0.0.1:{port}").parse().unwrap(), "doubleslash")
            .unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    let bytes = tokio::time::timeout(Duration::from_secs(3), async {
        connection
            .accept_uni()
            .await
            .unwrap()
            .read_to_end(4096)
            .await
            .unwrap()
    })
    .await
    .unwrap();
    let (welcome, _) = wire::decode_relay_cmd(&bytes).unwrap();
    assert_eq!(welcome["relay_cmd"], "welcome");
    (connection, welcome["index"].as_u64().unwrap() as u8)
}

async fn received(connection: &quinn::Connection, index: u8, body: &[u8]) {
    let packet = tokio::time::timeout(Duration::from_secs(3), connection.read_datagram())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(packet.as_ref(), wire::build_forwarded_datagram(index, body));
}

#[tokio::test]
async fn same_identity_devices_route_reconnect_and_disconnect_independently() {
    let root = SigningKey::generate(&mut rand::thread_rng());
    let public = root.verifying_key().to_bytes();
    let id = crate::crypto::b64url_encode(&public);
    let server = QUICRelayServer::new("test-server".into(), tests::test_features());
    server.state.write().device_routing_ready = true;
    server.allow_peer(&id);
    let port = server.start("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let desktop_device = DeviceId([1; 32]);
    let phone_device = DeviceId([2; 32]);
    let desktop_endpoint = client(&root, &public, desktop_device);
    let phone_endpoint = client(&root, &public, phone_device);
    let (desktop, desktop_index) = connect(&desktop_endpoint, port).await;
    let (phone, phone_index) = connect(&phone_endpoint, port).await;
    assert_ne!(desktop_index, phone_index);
    assert_eq!(server.stats().peers_connected, 2);
    assert!(desktop.close_reason().is_none());

    server.join_room_endpoint(&id, Some(desktop_device), "desktop-room");
    server.join_room_endpoint(&id, Some(phone_device), "phone-room");
    desktop
        .send_datagram(Bytes::from(wire::build_forwarded_datagram(
            phone_index,
            b"wrong-room",
        )))
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(80), phone.read_datagram())
            .await
            .is_err()
    );
    server.join_room_endpoint(&id, Some(phone_device), "desktop-room");
    assert_eq!(server.get_room_peers("desktop-room"), vec![id.clone()]);
    desktop
        .send_datagram(Bytes::from(wire::build_forwarded_datagram(
            phone_index,
            b"to-phone",
        )))
        .unwrap();
    received(&phone, desktop_index, b"to-phone").await;
    phone
        .send_datagram(Bytes::from(wire::build_forwarded_datagram(
            255,
            b"to-desktop",
        )))
        .unwrap();
    received(&desktop, phone_index, b"to-desktop").await;

    let replacement_endpoint = client(&root, &public, phone_device);
    let (replacement, replacement_index) = connect(&replacement_endpoint, port).await;
    tokio::time::timeout(Duration::from_secs(3), phone.closed())
        .await
        .unwrap();
    assert!(desktop.close_reason().is_none());
    assert_eq!(server.stats().peers_connected, 2);
    cleanup_stale_peers(&server.state, &server.features);
    assert_eq!(server.stats().peers_connected, 2);
    replacement
        .send_datagram(Bytes::from(wire::build_forwarded_datagram(
            desktop_index,
            b"after-reconnect",
        )))
        .unwrap();
    received(&desktop, replacement_index, b"after-reconnect").await;

    assert_eq!(
        server.send_feature_datagram(&format!("{id}="), "game.relay.v1", b"fanout"),
        Some(true)
    );
    for connection in [&desktop, &replacement] {
        let packet = tokio::time::timeout(Duration::from_secs(3), connection.read_datagram())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(packet.as_ref(), b"fanout");
    }
    replacement.close(0u32.into(), b"phone-offline");
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if server.stats().peers_connected == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(server.get_room_peers("desktop-room"), vec![id.clone()]);
    assert!(desktop.close_reason().is_none());
    server.revoke_peer(&id);
    tokio::time::timeout(Duration::from_secs(3), desktop.closed())
        .await
        .unwrap();
    assert_eq!(server.stats().peers_connected, 0);
    server.shutdown();
}

#[tokio::test]
async fn device_certificate_requires_root_possession_and_explicit_activation() {
    let root = SigningKey::generate(&mut rand::thread_rng());
    let public = root.verifying_key().to_bytes();
    let id = crate::crypto::b64url_encode(&public);
    let server = QUICRelayServer::new("test-server".into(), tests::test_features());
    server.state.write().device_routing_ready = false;
    server.allow_peer(&id);
    let port = server.start("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let valid = client(&root, &public, DeviceId([1; 32]));
    let connection = valid
        .connect(format!("127.0.0.1:{port}").parse().unwrap(), "doubleslash")
        .unwrap()
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), connection.closed())
        .await
        .unwrap();
    assert_eq!(server.stats().peers_connected, 0);
    server.state.write().device_routing_ready = true;
    let (authorized, _) = connect(&valid, port).await;
    let attacker = SigningKey::generate(&mut rand::thread_rng());
    let spoof = client(&attacker, &public, DeviceId([1; 32]));
    let attempt = tokio::time::timeout(
        Duration::from_secs(3),
        spoof
            .connect(format!("127.0.0.1:{port}").parse().unwrap(), "doubleslash")
            .unwrap(),
    )
    .await
    .unwrap();
    if let Ok(connection) = attempt {
        tokio::time::timeout(Duration::from_secs(3), connection.closed())
            .await
            .unwrap();
    }
    assert!(authorized.close_reason().is_none());
    assert_eq!(server.stats().peers_connected, 1);
    server.revoke_peer(&id);
    server.shutdown();
}
