//! QUIC TLS helpers for DoubleSlash peer-to-peer transport.
//!
//! Mirrors the cert generation in `doubleslash-quic/src/identity.rs`:
//! - Self-signed Ed25519 certificate, CN = hex(public_key_bytes).
//! - Both client and server use mutual TLS; cert verified by peer_id match.
//! - ALPN = b"doubleslash/1".

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, PKCS_ED25519};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};

/// ALPN protocol tag — must match the supernode and other peers.
pub const ALPN: &[u8] = b"doubleslash/1";

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a dual-stack Quinn `Endpoint` on `[::]:<port>` (0 for ephemeral).
/// Falls back to IPv4 when a dual-stack socket cannot be bound.
///
/// The endpoint uses the caller's Ed25519 identity for its TLS certificate.
/// Both inbound and outbound connections use mutual TLS; certificate
/// verification is handled by `AcceptAnyCert` / `AcceptAnyClient` which
/// accept any valid cert — callers must verify the peer_id from the cert CN.
pub fn make_quic_endpoint(signing_key: &SigningKey, port: u16) -> anyhow::Result<quinn::Endpoint> {
    make_quic_endpoint_with_device(signing_key, port, None)
}

pub fn make_quic_endpoint_with_device(
    signing_key: &SigningKey,
    port: u16,
    device: Option<doubleslash_features::DeviceId>,
) -> anyhow::Result<quinn::Endpoint> {
    make_quic_endpoint_tuned(signing_key, port, device, |_| {})
}

/// [`make_quic_endpoint_with_device`] with a hook to adjust the transport
/// after the production timings are applied — tests use it to pin the path
/// MTU at the QUIC floor, which loopback would otherwise probe past.
fn make_quic_endpoint_tuned(
    signing_key: &SigningKey,
    port: u16,
    device: Option<doubleslash_features::DeviceId>,
    tune: impl FnOnce(&mut quinn::TransportConfig),
) -> anyhow::Result<quinn::Endpoint> {
    let (cert_chain, key_der) = generate_self_signed_cert(signing_key, device)?;

    let provider = Arc::new(rustls::crypto::ring::default_provider());

    // -- server config (accepts incoming connections) -----------------------
    let server_config = {
        let verifier = Arc::new(AcceptAnyClient);
        let mut cfg = rustls::ServerConfig::builder_with_provider(Arc::clone(&provider))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| anyhow::anyhow!("TLS1.3 not supported: {e}"))?
            .with_client_cert_verifier(verifier)
            .with_single_cert(cert_chain.clone(), clone_key(&key_der))
            .map_err(|e| anyhow::anyhow!("Server cert: {e}"))?;
        cfg.alpn_protocols = vec![ALPN.to_vec()];
        cfg
    };

    // -- client config (for outgoing connections) ---------------------------
    let client_config = {
        let verifier = Arc::new(AcceptAnyCert);
        let mut cfg = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| anyhow::anyhow!("TLS1.3 not supported: {e}"))?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_client_auth_cert(cert_chain, key_der)
            .map_err(|e| anyhow::anyhow!("Client cert: {e}"))?;
        cfg.alpn_protocols = vec![ALPN.to_vec()];
        cfg
    };

    // -- transport timings --------------------------------------------------
    let transport = Arc::new({
        let mut t = quinn::TransportConfig::default();
        t.max_idle_timeout(Some(
            Duration::from_secs(120)
                .try_into()
                .map_err(|_| anyhow::anyhow!("idle timeout overflow"))?,
        ));
        t.keep_alive_interval(Some(Duration::from_secs(5)));
        tune(&mut t);
        t
    });

    let mut server_cfg = quinn::ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(server_config)
            .map_err(|e| anyhow::anyhow!("QuicServerConfig: {e}"))?,
    ));
    server_cfg.transport_config(Arc::clone(&transport));

    let mut client_cfg = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(client_config)
            .map_err(|e| anyhow::anyhow!("QuicClientConfig: {e}"))?,
    ));
    client_cfg.transport_config(Arc::clone(&transport));

    // -- bind endpoint ------------------------------------------------------
    let socket = bind_quic_socket(port, bind_dual_stack_socket)?;
    let runtime =
        quinn::default_runtime().ok_or_else(|| std::io::Error::other("no async runtime found"))?;
    let mut endpoint = quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(server_cfg),
        socket,
        runtime,
    )?;
    endpoint.set_default_client_config(client_cfg);
    Ok(endpoint)
}

fn bind_quic_socket(
    port: u16,
    bind_ipv6: impl FnOnce(u16) -> std::io::Result<UdpSocket>,
) -> std::io::Result<UdpSocket> {
    bind_ipv6(port).or_else(|error| {
        tracing::debug!(%error, "dual-stack QUIC bind failed; trying IPv4");
        UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))
    })
}

fn bind_dual_stack_socket(port: u16) -> std::io::Result<UdpSocket> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV6,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    // Windows defaults to IPv6-only. If this option fails, use the IPv4
    // fallback rather than silently losing connectivity to IPv4 peers.
    socket.set_only_v6(false)?;
    socket.bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)).into())?;
    Ok(socket.into())
}

/// Numeric QUIC hosts may come from a parsed hint (bare IPv6) or a relay
/// ticket (bracketed IPv6). Keep both dialers consistent without DNS work on
/// the manager loop.
pub(crate) fn quic_socket_addr(
    host: &str,
    port: u16,
) -> Result<SocketAddr, std::net::AddrParseError> {
    match host.parse::<IpAddr>() {
        Ok(ip) => Ok(SocketAddr::new(ip, port)),
        Err(_) => format!("{host}:{port}").parse(),
    }
}

/// Derive `peer_id` from raw Ed25519 public key bytes: `hex(sha256(pub_bytes))`.
pub fn peer_id_from_pub_bytes(pub_bytes: &[u8]) -> String {
    let hash = Sha256::digest(pub_bytes);
    hex::encode(hash)
}

/// Extract the hex-encoded CN from a DER-encoded self-signed certificate.
pub fn cn_from_cert_der(cert_der: &CertificateDer<'_>) -> Option<String> {
    let cn = parse_cn_from_der(cert_der.as_ref())?;
    Some(
        cn.split_once('.')
            .map_or(cn.as_str(), |(identity, _)| identity)
            .to_owned(),
    )
}

/// Device route authenticated by the root's TLS CertificateVerify signature.
pub fn device_from_cert_der(
    cert_der: &CertificateDer<'_>,
) -> Option<doubleslash_features::DeviceId> {
    let cn = parse_cn_from_der(cert_der.as_ref())?;
    let (_, device) = cn.split_once('.')?;
    let bytes = crate::crypto::b64url_decode(device).ok()?;
    Some(doubleslash_features::DeviceId(bytes.try_into().ok()?))
}

/// Verify that the TLS handshake signature `dss` over `message` was produced
/// by the identity the certificate's CN claims (`hex(identity_pub)`, with an
/// optional `.b64(device_id)` suffix) — not merely by whatever key the peer
/// put in the certificate's own SubjectPublicKeyInfo. A self-signed cert lets
/// the presenter set CN to anyone's hex pubkey while signing with a key of
/// their own choosing; without this check, that forged CN becomes the
/// `peer_id` the connection is keyed under (see `cn_from_cert_der` callers).
fn verify_device_root_signature(
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &rustls::DigitallySignedStruct,
) -> Result<(), rustls::Error> {
    let cn = parse_cn_from_der(cert.as_ref()).ok_or(rustls::Error::InvalidCertificate(
        rustls::CertificateError::BadEncoding,
    ))?;
    let check = || -> Option<()> {
        let identity_hex = match cn.split_once('.') {
            Some((identity, _)) => {
                // Validate the device suffix shape before trusting the claim.
                device_from_cert_der(cert)?;
                identity
            }
            None => cn.as_str(),
        };
        let root: [u8; 32] = hex::decode(identity_hex).ok()?.try_into().ok()?;
        if dss.scheme != rustls::SignatureScheme::ED25519 {
            return None;
        }
        let key = ed25519_dalek::VerifyingKey::from_bytes(&root).ok()?;
        key.verify_strict(
            message,
            &ed25519_dalek::Signature::from_slice(dss.signature()).ok()?,
        )
        .ok()
    };
    check().ok_or(rustls::Error::InvalidCertificate(
        rustls::CertificateError::BadSignature,
    ))
}

// ---------------------------------------------------------------------------
// Cert generation
// ---------------------------------------------------------------------------

fn generate_self_signed_cert(
    signing_key: &SigningKey,
    device: Option<doubleslash_features::DeviceId>,
) -> anyhow::Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let pub_bytes = signing_key.verifying_key().as_bytes().to_vec();
    let mut cn = hex::encode(&pub_bytes);
    if let Some(device) = device {
        use base64::Engine;
        cn.push('.');
        cn.push_str(&base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(device.0));
    }

    let pkcs8 = build_ed25519_pkcs8(signing_key);
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(pkcs8));

    let key_pair = KeyPair::from_der_and_sign_algo(&key_der, &PKCS_ED25519)
        .map_err(|e| anyhow::anyhow!("KeyPair: {e}"))?;

    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, &cn);
    params.distinguished_name = dn;
    params.not_before = rcgen::date_time_ymd(2020, 1, 1);
    params.not_after = rcgen::date_time_ymd(2030, 1, 1);

    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| anyhow::anyhow!("Self-sign: {e}"))?;
    let cert_der = CertificateDer::from(cert.der().to_vec());
    Ok((vec![cert_der], key_der))
}

fn clone_key(key: &PrivateKeyDer<'static>) -> PrivateKeyDer<'static> {
    match key {
        PrivateKeyDer::Pkcs8(k) => {
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(k.secret_pkcs8_der().to_vec()))
        }
        PrivateKeyDer::Pkcs1(k) => PrivateKeyDer::Pkcs1(
            rustls::pki_types::PrivatePkcs1KeyDer::from(k.secret_pkcs1_der().to_vec()),
        ),
        PrivateKeyDer::Sec1(k) => PrivateKeyDer::Sec1(rustls::pki_types::PrivateSec1KeyDer::from(
            k.secret_sec1_der().to_vec(),
        )),
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------------------
// Custom verifiers — accept any cert; peer_id verified by caller
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct AcceptAnyCert;

impl rustls::client::danger::ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        verify_device_root_signature(message, cert, dss)?;
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[derive(Debug)]
struct AcceptAnyClient;

impl rustls::server::danger::ClientCertVerifier for AcceptAnyClient {
    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::server::danger::ClientCertVerified, rustls::Error> {
        Ok(rustls::server::danger::ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        verify_device_root_signature(message, cert, dss)?;
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }

    fn client_auth_mandatory(&self) -> bool {
        false // optional client cert — peer connects with or without identity
    }

    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }
}

// ---------------------------------------------------------------------------
// DER CN parser
// ---------------------------------------------------------------------------

fn parse_cn_from_der(der: &[u8]) -> Option<String> {
    // CN OID 2.5.4.3 → [0x55, 0x04, 0x03]
    let cn_oid = [0x55u8, 0x04, 0x03];
    let pos = der.windows(cn_oid.len()).position(|w| w == cn_oid)?;
    let after_oid = pos + cn_oid.len();
    if after_oid >= der.len() {
        return None;
    }
    let tag = der[after_oid];
    if tag != 0x0C && tag != 0x13 {
        return None;
    }
    let len_pos = after_oid + 1;
    if len_pos >= der.len() {
        return None;
    }
    let (value_len, consumed) = parse_der_length(&der[len_pos..])?;
    let value_start = len_pos + consumed;
    let value_end = value_start + value_len;
    if value_end > der.len() {
        return None;
    }
    std::str::from_utf8(&der[value_start..value_end])
        .ok()
        .map(|s| s.to_owned())
}

fn parse_der_length(data: &[u8]) -> Option<(usize, usize)> {
    if data.is_empty() {
        return None;
    }
    let first = data[0];
    if first < 0x80 {
        Some((first as usize, 1))
    } else {
        let num_bytes = (first & 0x7F) as usize;
        if num_bytes == 0 || data.len() < 1 + num_bytes {
            return None;
        }
        let mut len = 0usize;
        for &b in &data[1..1 + num_bytes] {
            len = (len << 8) | b as usize;
        }
        Some((len, 1 + num_bytes))
    }
}

// ---------------------------------------------------------------------------
// PKCS#8 encoding for Ed25519
// ---------------------------------------------------------------------------

fn build_ed25519_pkcs8(key: &SigningKey) -> Vec<u8> {
    let raw = key.to_bytes();
    let inner_octet = asn1_octet_string(&raw);
    let outer_octet = asn1_octet_string(&inner_octet);
    let oid_bytes = [0x06u8, 0x03, 0x2b, 0x65, 0x70]; // OID 1.3.101.112
    let algo_seq = asn1_sequence(&oid_bytes);
    let version = [0x02u8, 0x01, 0x00];
    let mut inner = Vec::new();
    inner.extend_from_slice(&version);
    inner.extend_from_slice(&algo_seq);
    inner.extend_from_slice(&outer_octet);
    asn1_sequence(&inner)
}

fn asn1_sequence(data: &[u8]) -> Vec<u8> {
    let mut buf = vec![0x30u8];
    encode_asn1_length(data.len(), &mut buf);
    buf.extend_from_slice(data);
    buf
}

fn asn1_octet_string(data: &[u8]) -> Vec<u8> {
    let mut buf = vec![0x04u8];
    encode_asn1_length(data.len(), &mut buf);
    buf.extend_from_slice(data);
    buf
}

fn encode_asn1_length(len: usize, buf: &mut Vec<u8>) {
    if len < 0x80 {
        buf.push(len as u8);
    } else if len < 0x100 {
        buf.extend_from_slice(&[0x81, len as u8]);
    } else {
        buf.extend_from_slice(&[0x82, (len >> 8) as u8, len as u8]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use doubleslash_features::DeviceId;

    #[test]
    fn endpoint_without_an_async_runtime_returns_an_error() {
        assert!(make_quic_endpoint(&SigningKey::from_bytes(&[67; 32]), 0).is_err());
    }

    #[test]
    fn quic_addresses_accept_ipv4_and_both_ipv6_host_spellings() {
        for (host, expected) in [
            ("192.0.2.1", "192.0.2.1:3478"),
            ("2001:db8::1", "[2001:db8::1]:3478"),
            ("[2001:db8::1]", "[2001:db8::1]:3478"),
        ] {
            assert_eq!(quic_socket_addr(host, 3478).unwrap().to_string(), expected);
        }
        for invalid in ["", "[::1", "127.0.0.1:99", "not an address"] {
            assert!(quic_socket_addr(invalid, 3478).is_err());
        }
    }

    /// Exercise both address families and reconnect on the same production
    /// endpoint. A Windows IPv6-only default would pass the IPv6 half but
    /// strand every IPv4 peer; the old IPv4 bind fails the IPv6 half.
    #[tokio::test]
    async fn one_endpoint_carries_datagrams_and_reconnects_over_ipv4_and_ipv6() {
        let server = make_quic_endpoint(&SigningKey::from_bytes(&[61; 32]), 0).unwrap();
        let client = make_quic_endpoint(&SigningKey::from_bytes(&[62; 32]), 0).unwrap();
        assert!(server.local_addr().unwrap().is_ipv6());
        assert!(client.local_addr().unwrap().is_ipv6());
        let port = server.local_addr().unwrap().port();

        for host in ["127.0.0.1", "::1"] {
            for _ in 0..2 {
                tokio::time::timeout(Duration::from_secs(5), async {
                    let connecting = client
                        .connect(quic_socket_addr(host, port).unwrap(), "doubleslash")
                        .unwrap();
                    let accepting = async { server.accept().await.unwrap().await.unwrap() };
                    let (outbound, inbound) = tokio::join!(connecting, accepting);
                    let outbound = outbound.unwrap();
                    outbound
                        .send_datagram(b"to server".to_vec().into())
                        .unwrap();
                    assert_eq!(
                        inbound.read_datagram().await.unwrap().as_ref(),
                        b"to server"
                    );
                    inbound.send_datagram(b"to client".to_vec().into()).unwrap();
                    assert_eq!(
                        outbound.read_datagram().await.unwrap().as_ref(),
                        b"to client"
                    );
                    outbound.close(0u32.into(), b"reconnect");
                    inbound.closed().await;
                })
                .await
                .unwrap();
            }
        }
        server.close(0u32.into(), b"test complete");
        client.close(0u32.into(), b"test complete");
    }

    #[test]
    fn unavailable_dual_stack_bind_falls_back_to_ipv4_on_the_same_port() {
        let unavailable = |_: u16| Err(std::io::Error::from(std::io::ErrorKind::Unsupported));
        let fallback = bind_quic_socket(0, unavailable).unwrap();
        let port = fallback.local_addr().unwrap().port();
        assert_eq!(
            fallback.local_addr().unwrap(),
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))
        );
        // A failed fallback must not silently choose another listener port.
        assert!(bind_quic_socket(port, unavailable).is_err());
        drop(fallback);
        let fallback = bind_quic_socket(port, unavailable).unwrap();
        assert_eq!(fallback.local_addr().unwrap().port(), port);
        fallback
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        sender
            .send_to(b"fallback", (Ipv4Addr::LOCALHOST, port))
            .unwrap();
        let mut received = [0; 8];
        assert_eq!(fallback.recv(&mut received).unwrap(), 8);
        assert_eq!(&received, b"fallback");
    }

    /// A path that never grew past the QUIC floor — a phone on a mobile
    /// network — must still carry a full video fragment. Before this was
    /// pinned, fragments were sized for the sender's grown path and every one
    /// was refused on such a link, leaving the picture black while voice (small
    /// datagrams) kept working.
    #[tokio::test]
    async fn a_portable_video_fragment_fits_a_path_held_at_the_quic_floor() {
        let floor = |t: &mut quinn::TransportConfig| {
            t.initial_mtu(1200);
            t.min_mtu(1200);
            t.mtu_discovery_config(None);
        };
        let a =
            make_quic_endpoint_tuned(&SigningKey::from_bytes(&[51; 32]), 0, None, floor).unwrap();
        let b =
            make_quic_endpoint_tuned(&SigningKey::from_bytes(&[52; 32]), 0, None, floor).unwrap();
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], a.local_addr().unwrap().port()));
        let connect = b.connect(address, "doubleslash").unwrap();
        let accept = async { a.accept().await.unwrap().await.unwrap() };
        let (client, server) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(connect, accept)
        })
        .await
        .unwrap();
        let client = client.unwrap();

        let max = client.max_datagram_size().unwrap();
        assert!(
            max >= crate::video::PORTABLE_MAX_DATAGRAM,
            "a floor path carries {max}B; the portable size must not exceed it"
        );
        // The old direct-video size — a whole 1200-byte UDP payload — does not
        // fit in a QUIC datagram on this path.
        assert!(max < 1200);

        client
            .send_datagram(vec![0xA5; crate::video::PORTABLE_MAX_DATAGRAM].into())
            .expect("a portable-size datagram is accepted on a floor path");
        let got = tokio::time::timeout(Duration::from_secs(5), server.read_datagram())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.len(), crate::video::PORTABLE_MAX_DATAGRAM);

        a.close(0u32.into(), b"test complete");
        b.close(0u32.into(), b"test complete");
    }

    #[tokio::test]
    async fn direct_tls_authenticates_distinct_devices_under_one_root() {
        let key = SigningKey::from_bytes(&[31; 32]);
        let desktop = make_quic_endpoint_with_device(&key, 0, Some(DeviceId([1; 32]))).unwrap();
        let phone = make_quic_endpoint_with_device(&key, 0, Some(DeviceId([2; 32]))).unwrap();
        let address =
            std::net::SocketAddr::from(([127, 0, 0, 1], desktop.local_addr().unwrap().port()));
        let connect = phone.connect(address, "doubleslash").unwrap();
        let server = async { desktop.accept().await.unwrap().await.unwrap() };
        let (client, server) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(connect, server)
        })
        .await
        .unwrap();
        let client = client.unwrap();
        for (connection, device) in [(&client, DeviceId([1; 32])), (&server, DeviceId([2; 32]))] {
            let certs = connection
                .peer_identity()
                .unwrap()
                .downcast::<Vec<CertificateDer>>()
                .unwrap();
            assert_eq!(device_from_cert_der(&certs[0]), Some(device));
            assert_eq!(
                cn_from_cert_der(&certs[0]),
                Some(hex::encode(key.verifying_key().as_bytes()))
            );
        }
        desktop.close(0u32.into(), b"test complete");
        phone.close(0u32.into(), b"test complete");
    }

    #[tokio::test]
    async fn direct_tls_rejects_device_claim_signed_by_another_root() {
        let attacker = SigningKey::from_bytes(&[42; 32]);
        let owner = SigningKey::from_bytes(&[43; 32]);
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(build_ed25519_pkcs8(&attacker)));
        let pair = KeyPair::from_der_and_sign_algo(&key, &PKCS_ED25519).unwrap();
        let mut params = CertificateParams::default();
        params.distinguished_name.push(
            DnType::CommonName,
            format!(
                "{}.{}",
                hex::encode(owner.verifying_key().as_bytes()),
                crate::crypto::b64url_encode(&[1; 32])
            ),
        );
        let cert = params.self_signed(&pair).unwrap();
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.der().clone()], key)
        .unwrap();
        config.alpn_protocols = vec![ALPN.to_vec()];
        let config = quinn::ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(config).unwrap(),
        ));
        let server = quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let client = make_quic_endpoint(&owner, 0).unwrap();
        let connect = client
            .connect(server.local_addr().unwrap(), "doubleslash")
            .unwrap();
        let accept = async { server.accept().await.unwrap().await };
        let (client_result, server_result) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(connect, accept)
        })
        .await
        .unwrap();
        assert!(client_result.is_err());
        assert!(server_result.is_err());
    }

    #[tokio::test]
    async fn direct_tls_rejects_legacy_claim_signed_by_another_root() {
        // No device suffix on the CN — the plain identity-cert shape every
        // client used before device routing existed. An attacker self-signs
        // with their own key but sets CN to the victim's hex pubkey; the
        // handshake must fail rather than connecting under the victim's id.
        let attacker = SigningKey::from_bytes(&[44; 32]);
        let victim = SigningKey::from_bytes(&[45; 32]);
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(build_ed25519_pkcs8(&attacker)));
        let pair = KeyPair::from_der_and_sign_algo(&key, &PKCS_ED25519).unwrap();
        let mut params = CertificateParams::default();
        params.distinguished_name.push(
            DnType::CommonName,
            hex::encode(victim.verifying_key().as_bytes()),
        );
        let cert = params.self_signed(&pair).unwrap();
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.der().clone()], key)
        .unwrap();
        config.alpn_protocols = vec![ALPN.to_vec()];
        let config = quinn::ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(config).unwrap(),
        ));
        let server = quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let client = make_quic_endpoint(&victim, 0).unwrap();
        let connect = client
            .connect(server.local_addr().unwrap(), "doubleslash")
            .unwrap();
        let accept = async { server.accept().await.unwrap().await };
        let (client_result, server_result) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(connect, accept)
        })
        .await
        .unwrap();
        assert!(client_result.is_err());
        assert!(server_result.is_err());
    }
}
