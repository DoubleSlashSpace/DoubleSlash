//! Supernode WebSocket background tasks.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{mpsc, Notify};
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::{debug, info, warn};

use serde_json::Value;

use crate::connection_fallback::{
    classify_signaling_hint, dddns_endpoints_from_json, is_dddns_lookup_name, signaling_url_for,
    SignalingHint,
};
use crate::identity::Identity;
use crate::protocol::{MessageType, SignalingMessage};

use super::internal::InternalEvent;

const PING_INTERVAL_S: u64 = 30;
/// Base reconnect delay after a successful session ends (clean close or mid-session error).
const RECONNECT_BASE: Duration = Duration::from_secs(1);
/// Cap on exponential backoff after repeated connect failures.
const RECONNECT_MAX: Duration = Duration::from_secs(60);
/// Cap on a single dial (TCP + TLS + HTTP upgrade).
///
/// Without it a connect issued against a half-up interface — the normal state
/// for a second or two after a phone changes network — inherits the OS connect
/// timeout, which is minutes. The task would sit in `connect_async` with no
/// retry and no `WsDisconnected`, so the client looks connected while nothing
/// flows.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Bound on a DNS or DDDNS lookup that runs only after an IP dial has failed.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);
/// Largest DDDNS response read. A v1 record is well under 1 KiB.
const MAX_DDDNS_RECORD_BYTES: usize = 4096;
/// How long a supernode has to sign back our HELLO's challenge. Until it does,
/// nothing but the HELLO is sent and the session is not reported connected.
#[cfg(not(test))]
const HELLO_PROOF_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
const HELLO_PROOF_TIMEOUT: Duration = Duration::from_millis(500);
/// Messages a supernode may send before its proof, held until it arrives.
const MAX_MESSAGES_BEFORE_PROOF: usize = 64;
/// Same window as the manager's `MAX_MESSAGE_AGE_SECS` for signed signaling.
const HELLO_PROOF_MAX_AGE_SECS: f64 = 300.0;
/// How long a session may receive *nothing* before we treat the link as dead.
///
/// The manager pings every connected supernode every 30 s and the supernode
/// answers with a signed `Pong`, so a healthy link is never silent for longer
/// than one ping period plus RTT — idle or not. Three silent periods therefore
/// mean the path is gone rather than quiet.
///
/// This is what makes a network change recoverable. A TCP socket whose source
/// address has vanished (Wi-Fi to cellular) neither errors nor delivers: reads
/// park forever and writes disappear into the send buffer. With no deadline the
/// task waits in `ws_stream.next()` indefinitely, never emits `WsDisconnected`,
/// and — because relay teardown and re-dial both hang off that event — the
/// whole client stays dark until it is restarted.
#[cfg(not(test))]
const READ_IDLE_TIMEOUT: Duration = Duration::from_secs(95);
/// Same deadline, shortened so the tests exercise the mechanism in real time.
/// The production value is a policy choice about how quiet a healthy supernode
/// may be; what the tests need to pin down is that the deadline fires at all,
/// that inbound traffic defers it, and that the task recovers afterwards.
#[cfg(test)]
const READ_IDLE_TIMEOUT: Duration = Duration::from_millis(300);
/// Settle delay before re-dialing on a network change, so the dial lands after
/// the new route is installed rather than racing it.
const NETWORK_CHANGE_SETTLE: Duration = Duration::from_millis(250);

// ---------------------------------------------------------------------------
// Supernode WebSocket task
// ---------------------------------------------------------------------------

/// Why `connect_and_run_ws` returned.
#[derive(Debug, PartialEq, Eq)]
enum WsSessionEnd {
    /// Manager dropped the outbound channel — stop the task (no reconnect).
    ChannelClosed,
    /// Remote closed the socket (or EOF). Reconnect with a short delay.
    RemoteClosed,
    /// The platform reported the network changed under this socket. Re-dial the
    /// *same* candidate at once: the endpoint is fine, only our local address
    /// moved.
    NetworkChanged,
}

/// Long-running tokio task that maintains a WebSocket connection to a
/// supernode, rotating through ordered `candidates` on failure.
///
/// A candidate whose host is an IP is dialed as written. A hostname is used
/// only after that fails: an ordinary name is resolved with DNS, and a name
/// under `*.doubleslash.space` is fetched from DDDNS over HTTPS. Both paths
/// dial the resulting IP. A lookup that fails is skipped, and the task goes
/// back to the invite address.
///
/// Reconnects on both clean remote close and I/O errors until the manager
/// drops the outbound send channel (intentional teardown) or the task is
/// aborted.
///
/// `reconnect_now` is the platform's "the network moved" signal, raised by
/// `ConnectionCommand::NetworkChanged`. It cuts short both a live session and a
/// backoff sleep, so a phone that has just landed on a working interface
/// re-dials immediately instead of sitting out a delay earned on a dead one.
pub(super) async fn supernode_ws_task(
    identity: Arc<Identity>,
    device_id: Option<doubleslash_features::DeviceId>,
    peer_id: String,
    candidates: Arc<Mutex<Vec<String>>>,
    mut send_rx: mpsc::Receiver<WsMessage>,
    internal_tx: mpsc::Sender<InternalEvent>,
    reconnect_now: Arc<Notify>,
) {
    let initial_empty = candidates
        .lock()
        .map(|list| list.is_empty())
        .unwrap_or(true);
    if initial_empty {
        warn!(
            "Supernode {} has no WebSocket candidates — task exiting",
            peer_id
        );
        return;
    }

    let mut backoff = RECONNECT_BASE;
    let mut candidate_idx: usize = 0;

    'session: loop {
        let hint = {
            let list = candidates
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if list.is_empty() {
                warn!(
                    "Supernode {} has no WebSocket candidates — task exiting",
                    peer_id
                );
                return;
            }
            list[candidate_idx % list.len()].clone()
        };
        let targets = dial_targets_for(&hint).await;
        if targets.is_empty() {
            warn!(
                "Supernode {} hint {hint} did not resolve to an address",
                peer_id
            );
            candidate_idx = candidate_idx.wrapping_add(1);
            let (wait, next) = backoff_after_error(backoff, false);
            backoff = next;
            wait_before_retry(wait, &reconnect_now).await;
            continue;
        }

        let mut target_idx = 0;
        while target_idx < targets.len() {
            let ws_url = &targets[target_idx];
            info!("Connecting to supernode {} at {ws_url}", peer_id);
            // Whether this attempt reached a live session. Separates "this endpoint
            // is wrong" from "the link died under a working endpoint".
            let mut established = false;
            let outcome = connect_and_run_ws(
                (&identity, device_id),
                &peer_id,
                ws_url,
                &mut send_rx,
                &internal_tx,
                &reconnect_now,
                &mut established,
            )
            .await;
            match outcome {
                Ok(WsSessionEnd::ChannelClosed) => {
                    info!(
                        "Supernode {} WebSocket task stopping (outbound channel closed)",
                        peer_id
                    );
                    let _ = internal_tx
                        .send(InternalEvent::WsDisconnected {
                            peer_id: peer_id.clone(),
                        })
                        .await;
                    break 'session;
                }
                Ok(WsSessionEnd::NetworkChanged) => {
                    info!(
                        "Supernode {} network changed; re-dialing {ws_url} immediately",
                        peer_id
                    );
                    let _ = internal_tx
                        .send(InternalEvent::WsDisconnected {
                            peer_id: peer_id.clone(),
                        })
                        .await;
                    // Same address, no backoff: nothing is wrong with the
                    // endpoint, so rotating away from it would be a downgrade.
                    backoff = RECONNECT_BASE;
                    sleep(NETWORK_CHANGE_SETTLE).await;
                }
                Ok(WsSessionEnd::RemoteClosed) => {
                    info!(
                        "Supernode {} WebSocket closed; reconnecting in {:?}",
                        peer_id, RECONNECT_BASE
                    );
                    let _ = internal_tx
                        .send(InternalEvent::WsDisconnected {
                            peer_id: peer_id.clone(),
                        })
                        .await;
                    // Rotate candidate after a successful session ends so we probe
                    // alternate endpoints if the primary is flapping.
                    candidate_idx = candidate_idx.wrapping_add(1);
                    backoff = RECONNECT_BASE;
                    wait_before_retry(RECONNECT_BASE, &reconnect_now).await;
                    break;
                }
                Err(e) => {
                    warn!("Supernode {} WebSocket error on {ws_url}: {e}", peer_id);
                    let _ = internal_tx
                        .send(InternalEvent::WsDisconnected {
                            peer_id: peer_id.clone(),
                        })
                        .await;
                    // Another address from the same lookup (the other family)
                    // is tried before the next stored candidate and its backoff.
                    if !established && target_idx + 1 < targets.len() {
                        target_idx += 1;
                        continue;
                    }
                    let (wait, next) = backoff_after_error(backoff, established);
                    backoff = next;
                    warn!("Supernode {} retry in {:?}", peer_id, wait);
                    candidate_idx = candidate_idx.wrapping_add(1);
                    wait_before_retry(wait, &reconnect_now).await;
                    break;
                }
            }
        }
    }
}

/// IP WebSocket URLs for one stored hint.
///
/// An address is returned unchanged. An ordinary hostname is resolved with
/// DNS at the port in the hint. A DDDNS name, from a `dddns://` hint in any
/// zone or a name under `*.doubleslash.space`, is fetched over HTTPS
/// (`docs/DDDNS.md`), and the record's IP and port are what gets dialed. A
/// DDDNS name is never passed to DNS: that answers with the service's own
/// servers. Whatever answers must still prove it is the supernode
/// (`await_hello_proof`).
async fn dial_targets_for(hint: &str) -> Vec<String> {
    match classify_signaling_hint(hint) {
        SignalingHint::Direct => vec![hint.to_owned()],
        SignalingHint::Unusable => {
            warn!("ignoring unusable DDDNS hint {hint}");
            Vec::new()
        }
        SignalingHint::Dns { name, scheme, port } => lookup_dns(&name, scheme, port).await,
        SignalingHint::Dddns { name, scheme } => match lookup_dddns(&name).await {
            Ok(endpoints) => endpoints
                .into_iter()
                .filter_map(|(ip, port)| signaling_url_for(scheme, ip, port))
                .collect(),
            Err(error) => {
                warn!("DDDNS lookup for {name} failed: {error}");
                Vec::new()
            }
        },
    }
}

/// `scheme://ip:port` for each address DNS gives `name`.
async fn lookup_dns(name: &str, scheme: &str, port: u16) -> Vec<String> {
    match tokio::time::timeout(LOOKUP_TIMEOUT, tokio::net::lookup_host((name, port))).await {
        Ok(Ok(addresses)) => {
            let mut urls = Vec::new();
            for address in addresses {
                let Some(url) = signaling_url_for(scheme, address.ip(), port) else {
                    continue;
                };
                if !urls.contains(&url) {
                    urls.push(url);
                }
            }
            if urls.is_empty() {
                warn!("DNS for {name} returned no address");
            }
            urls
        }
        Ok(Err(error)) => {
            warn!("DNS for {name} failed: {error}");
            Vec::new()
        }
        Err(_) => {
            warn!("DNS for {name} timed out");
            Vec::new()
        }
    }
}

async fn lookup_dddns(name: &str) -> Result<Vec<(std::net::IpAddr, u16)>, String> {
    if !is_dddns_lookup_name(name) {
        return Err("not a name in a zone".into());
    }
    let url = format!("https://{name}/.well-known/doubleslash");
    let client = reqwest::Client::builder()
        .timeout(LOOKUP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    let mut response = client
        .get(&url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    // Any operator's host can be asked now, so read no more than a record.
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if body.len() + chunk.len() > MAX_DDDNS_RECORD_BYTES {
            return Err("response is larger than a record".into());
        }
        body.extend_from_slice(&chunk);
    }
    let body = String::from_utf8(body).map_err(|_| "record is not UTF-8".to_owned())?;
    dddns_endpoints_from_json(name, &body)
}

/// How long to wait before re-dialing after an errored attempt, and the backoff
/// to carry into the attempt after that.
///
/// `established` means the attempt reached a live session before it failed,
/// which proves the candidate itself is good — what died was the link under it.
/// Without that reset the backoff is only ever cleared by a *clean* close, so a
/// mobile client that changes network a few times — each change ending its
/// session with an error, not a close — climbs to the 60 s cap and stays there
/// for the rest of the process's life.
fn backoff_after_error(current: Duration, established: bool) -> (Duration, Duration) {
    let wait = if established { RECONNECT_BASE } else { current };
    (wait, (wait * 2).min(RECONNECT_MAX))
}

/// Sleep before the next dial, cut short by a network-change signal.
async fn wait_before_retry(delay: Duration, reconnect_now: &Notify) {
    tokio::select! {
        _ = sleep(delay) => {}
        _ = reconnect_now.notified() => {
            debug!("Network changed during backoff — dialing now");
        }
    }
}

async fn connect_and_run_ws(
    endpoint: (&Identity, Option<doubleslash_features::DeviceId>),
    peer_id: &str,
    ws_url: &str,
    send_rx: &mut mpsc::Receiver<WsMessage>,
    internal_tx: &mpsc::Sender<InternalEvent>,
    reconnect_now: &Notify,
    established: &mut bool,
) -> std::result::Result<WsSessionEnd, Box<dyn std::error::Error + Send + Sync>> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let mut request = ws_url.into_client_request()?;
    if endpoint.1.is_some() {
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            tokio_tungstenite::tungstenite::http::HeaderValue::from_static(
                doubleslash_features::device::DEVICE_WEBSOCKET_PROTOCOL,
            ),
        );
    }
    let connection = tokio::time::timeout(CONNECT_TIMEOUT, connect_async(request))
        .await
        .map_err(|_| format!("connect to {ws_url} timed out after {CONNECT_TIMEOUT:?}"))?;
    let (mut ws_stream, response) = match connection {
        Ok(connection) => connection,
        Err(error) => {
            if endpoint.1.is_some() && matches!(&error, tokio_tungstenite::tungstenite::Error::Protocol(
                tokio_tungstenite::tungstenite::error::ProtocolError::SecWebSocketSubProtocolError(_))) {
                let _ = internal_tx.send(InternalEvent::DeviceRoutingUnsupported { peer_id: peer_id.to_owned() }).await;
            }
            return Err(Box::new(error));
        }
    };
    if endpoint.1.is_some()
        && response
            .headers()
            .get("Sec-WebSocket-Protocol")
            .and_then(|value| value.to_str().ok())
            != Some(doubleslash_features::device::DEVICE_WEBSOCKET_PROTOCOL)
    {
        let _ = ws_stream.close(None).await;
        let _ = internal_tx
            .send(InternalEvent::DeviceRoutingUnsupported {
                peer_id: peer_id.to_owned(),
            })
            .await;
        return Err("This node needs an update for simultaneous identity use".into());
    }
    info!("WebSocket open to supernode {}", peer_id);
    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    // Send HELLO with a fresh challenge, then hold the session back until the
    // node signs it. The address came from an invite, DNS or DDDNS, and none
    // of those proves who answers; without this the client would replay rooms
    // and invite tokens to whatever listens there.
    let challenge = new_hello_challenge();
    let hello = build_hello(endpoint.0, endpoint.1, &challenge)?;
    ws_sink.send(WsMessage::Text(hello)).await?;
    let early = await_hello_proof(&mut ws_stream, peer_id, &challenge).await?;
    info!("Supernode {} proved its identity", peer_id);

    *established = true;
    let _ = internal_tx
        .send(InternalEvent::WsConnected {
            peer_id: peer_id.to_owned(),
        })
        .await;
    for msg in early {
        let _ = internal_tx
            .send(InternalEvent::WsSignalingMessage {
                supernode_id: peer_id.to_owned(),
                msg,
            })
            .await;
    }

    // I/O loop
    let mut ping_interval = tokio::time::interval(Duration::from_secs(PING_INTERVAL_S));
    // Delay, not Burst: after a stall we want one ping, not a catch-up volley.
    ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let idle_deadline = tokio::time::sleep(READ_IDLE_TIMEOUT);
    tokio::pin!(idle_deadline);

    loop {
        tokio::select! {
            // Outbound — `None` means the manager dropped send_tx (teardown).
            msg = send_rx.recv() => {
                match msg {
                    Some(msg) => {
                        ws_sink.send(msg).await?;
                    }
                    None => {
                        let _ = ws_sink.send(WsMessage::Close(None)).await;
                        return Ok(WsSessionEnd::ChannelClosed);
                    }
                }
            }

            // Inbound
            msg = ws_stream.next() => {
                // Any frame at all — Text, Pong, or the supernode's own Ping —
                // proves the path still carries traffic in our direction.
                idle_deadline
                    .as_mut()
                    .reset(tokio::time::Instant::now() + READ_IDLE_TIMEOUT);
                match msg {
                    Some(Ok(WsMessage::Text(text))) => {
                        match serde_json::from_str::<SignalingMessage>(&text) {
                            Ok(sm) => {
                                // Awaited send: inbound signaling must not be
                                // silently dropped when the manager is busy —
                                // backpressure the socket read instead.
                                let _ = internal_tx
                                    .send(InternalEvent::WsSignalingMessage {
                                        supernode_id: peer_id.to_owned(),
                                        msg: sm,
                                    })
                                    .await;
                            }
                            Err(e) => {
                                debug!("Ignoring non-signaling WS message: {}", e);
                            }
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => {
                        return Ok(WsSessionEnd::RemoteClosed);
                    }
                    Some(Ok(_)) => {} // binary, ping, pong — ignore
                    Some(Err(e)) => {
                        return Err(Box::new(e));
                    }
                }
            }

            // Keepalive
            _ = ping_interval.tick() => {
                ws_sink.send(WsMessage::Ping(vec![])).await?;
            }

            // Liveness. See READ_IDLE_TIMEOUT: this is the only thing that
            // rescues a socket stranded on a network we no longer have.
            _ = &mut idle_deadline => {
                return Err(format!(
                    "no traffic from supernode {peer_id} for {READ_IDLE_TIMEOUT:?} — link presumed dead"
                )
                .into());
            }

            // The platform told us the network moved. Do not wait for the
            // deadline above; the current socket is already worthless.
            _ = reconnect_now.notified() => {
                return Ok(WsSessionEnd::NetworkChanged);
            }
        }
    }
}

/// 32 random bytes, base64url: the challenge a supernode signs back.
fn new_hello_challenge() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    crate::crypto::b64url_encode_nopad(&bytes)
}

/// Read until the supernode's `HelloProof` for `challenge` arrives. Returns
/// the signaling messages that came before it, to deliver once it has.
///
/// The session fails, and the WebSocket task moves on, when the node sends no
/// valid proof in time, sends one signed by another key, or closes. Each
/// failure means this address does not answer as `expected`: an old supernode
/// that needs an update, or a different machine.
async fn await_hello_proof<S>(
    stream: &mut S,
    expected: &str,
    challenge: &str,
) -> std::result::Result<Vec<SignalingMessage>, Box<dyn std::error::Error + Send + Sync>>
where
    S: futures_util::Stream<
            Item = std::result::Result<WsMessage, tokio_tungstenite::tungstenite::Error>,
        > + Unpin,
{
    use futures_util::StreamExt;
    let deadline = tokio::time::Instant::now() + HELLO_PROOF_TIMEOUT;
    let mut early = Vec::new();
    loop {
        let next = tokio::time::timeout_at(deadline, stream.next())
            .await
            .map_err(|_| {
                format!(
                    "supernode {expected} sent no identity proof within {HELLO_PROOF_TIMEOUT:?}: \
                     it may need an update, or this address is not that node"
                )
            })?;
        match next {
            Some(Ok(WsMessage::Text(text))) => {
                let Ok(msg) = serde_json::from_str::<SignalingMessage>(&text) else {
                    continue;
                };
                if msg.msg_type == MessageType::HelloProof {
                    if is_valid_hello_proof(&msg, expected, challenge) {
                        return Ok(early);
                    }
                    return Err(format!(
                        "the node at this address is not supernode {expected}: its identity proof \
                         does not verify"
                    )
                    .into());
                }
                if early.len() >= MAX_MESSAGES_BEFORE_PROOF {
                    return Err(format!(
                        "supernode {expected} sent too much before its identity proof"
                    )
                    .into());
                }
                early.push(msg);
            }
            Some(Ok(WsMessage::Close(_))) | None => {
                return Err(
                    format!("supernode {expected} closed before proving its identity").into(),
                );
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(Box::new(e)),
        }
    }
}

/// A `HelloProof` from `expected` (padding ignored), signed by that key,
/// fresh, and carrying our `challenge`.
fn is_valid_hello_proof(msg: &SignalingMessage, expected: &str, challenge: &str) -> bool {
    if msg.sender.trim_end_matches('=') != expected.trim_end_matches('=') {
        return false;
    }
    if msg.payload.get("challenge").and_then(Value::as_str) != Some(challenge) {
        return false;
    }
    let Some(sig) = msg
        .signature
        .as_deref()
        .and_then(|s| crate::crypto::b64url_decode(s).ok())
    else {
        return false;
    };
    let Ok(key) = crate::crypto::b64url_decode(&msg.sender) else {
        return false;
    };
    let Ok(canonical) = msg.canonical_bytes() else {
        return false;
    };
    key.len() == 32
        && crate::crypto::ed25519_verify(&key, &sig, &canonical)
        && msg.is_fresh(HELLO_PROOF_MAX_AGE_SECS)
}

pub(super) fn build_hello(
    identity: &Identity,
    device_id: Option<doubleslash_features::DeviceId>,
    challenge: &str,
) -> std::result::Result<String, serde_json::Error> {
    let sender = identity.public_id();
    let mut msg = SignalingMessage::new(MessageType::Hello, sender.clone());
    msg.source_device = device_id;
    if device_id.is_some() {
        msg.payload
            .insert("device_routing".to_owned(), Value::from(1));
    }
    msg.payload
        .insert("public_id".to_owned(), Value::String(sender.clone()));
    msg.payload
        .insert("peer_id".to_owned(), Value::String(identity.peer_id()));
    // The supernode signs this back as a `HelloProof`.
    msg.payload
        .insert("challenge".to_owned(), Value::String(challenge.to_owned()));
    // Sign
    if let Ok(canonical) = msg.canonical_bytes() {
        let sig = identity.sign(&canonical);
        use base64::Engine;
        msg.signature = Some(base64::engine::general_purpose::URL_SAFE.encode(sig));
    }
    msg.to_json()
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    #[tokio::test]
    async fn an_ip_hint_is_dialed_unchanged() {
        let url = "ws://127.0.0.1:34935";
        assert_eq!(dial_targets_for(url).await, vec![url.to_owned()]);
        let v6 = "ws://[2001:db8::7]:34935";
        assert_eq!(dial_targets_for(v6).await, vec![v6.to_owned()]);
    }

    #[tokio::test]
    async fn device_client_rejects_old_node_before_registering_identity() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut websocket = accept_async(socket).await.unwrap();
            let first = tokio::time::timeout(Duration::from_secs(3), websocket.next())
                .await
                .unwrap();
            assert!(
                !matches!(first, Some(Ok(WsMessage::Text(_)))),
                "identity must not register on a legacy node"
            );
        });
        let identity = Identity::generate();
        let (_send, mut outbound) = mpsc::channel(8);
        let (events, mut incoming) = mpsc::channel(8);
        let mut established = false;
        let result = connect_and_run_ws(
            (&identity, Some(doubleslash_features::DeviceId([1; 32]))),
            "old-node",
            &url,
            &mut outbound,
            &events,
            &Notify::new(),
            &mut established,
        )
        .await;
        assert!(result.is_err());
        assert!(!established);
        assert!(matches!(
            incoming.try_recv(),
            Ok(InternalEvent::DeviceRoutingUnsupported { .. })
        ));
        server.await.unwrap();
    }

    /// What a test server does with each connection after the handshake.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ServerBehaviour {
        /// Accept, then say nothing at all — the shape of a link stranded on a
        /// network the device no longer has. The socket is open, the peer is
        /// gone, and nothing will ever arrive.
        Silent,
        /// Accept, then send a frame well inside the idle deadline, the way a
        /// live supernode answers the periodic Ping with a Pong.
        Chatty,
    }

    /// Serve `behaviour` forever on loopback as `node`, reporting each
    /// accepted WebSocket handshake on the returned channel.
    async fn serve(behaviour: ServerBehaviour) -> (String, mpsc::Receiver<()>) {
        serve_as(behaviour, node_identity(), ProofBehaviour::Valid).await
    }

    /// The identity the fake node proves, shared with [`spawn_task`].
    fn node_identity() -> Arc<Identity> {
        static NODE: std::sync::OnceLock<Arc<Identity>> = std::sync::OnceLock::new();
        Arc::clone(NODE.get_or_init(|| Arc::new(Identity::generate())))
    }

    /// How the fake node answers our HELLO's challenge.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ProofBehaviour {
        Valid,
        /// Signs with a key that is not the node's.
        WrongKey,
        /// Signs a different challenge.
        WrongChallenge,
        /// Never answers, like a supernode from before the proof.
        None,
    }

    /// The fake node's `HelloProof` for `hello`, or `None` when it sends none.
    fn proof_for(hello: &str, node: &Identity, proof: ProofBehaviour) -> Option<String> {
        let hello: SignalingMessage = serde_json::from_str(hello).ok()?;
        let mut challenge = hello.payload.get("challenge")?.as_str()?.to_owned();
        let stranger = Identity::generate();
        let signer: &Identity = match proof {
            ProofBehaviour::None => return None,
            ProofBehaviour::WrongKey => &stranger,
            ProofBehaviour::WrongChallenge => {
                challenge.push('x');
                node
            }
            ProofBehaviour::Valid => node,
        };
        let mut msg = SignalingMessage::new(MessageType::HelloProof, node.public_id());
        msg.payload
            .insert("challenge".to_owned(), Value::String(challenge));
        let canonical = msg.canonical_bytes().ok()?;
        msg.signature = Some(crate::crypto::b64url_encode(&signer.sign(&canonical)));
        msg.to_json().ok()
    }

    async fn serve_as(
        behaviour: ServerBehaviour,
        node: Arc<Identity>,
        proof: ProofBehaviour,
    ) -> (String, mpsc::Receiver<()>) {
        let (accepted_tx, accepted_rx) = mpsc::channel::<()>(16);
        // Bind before spawning so the caller has the address without having to
        // wait on the server task. Blocking the test thread for it would
        // deadlock the current-thread runtime the task needs to start on.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");

        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let accepted_tx = accepted_tx.clone();
                let node = Arc::clone(&node);
                tokio::spawn(async move {
                    let Ok(ws) = accept_async(stream).await else {
                        return;
                    };
                    let (mut sink, mut source) = ws.split();
                    let _ = accepted_tx.send(()).await;
                    // The first frame is the client's HELLO.
                    let Some(Ok(WsMessage::Text(hello))) = source.next().await else {
                        return;
                    };
                    if let Some(answer) = proof_for(&hello, &node, proof) {
                        if sink.send(WsMessage::Text(answer)).await.is_err() {
                            return;
                        }
                    }

                    if behaviour == ServerBehaviour::Chatty {
                        tokio::spawn(async move {
                            loop {
                                sleep(READ_IDLE_TIMEOUT / 3).await;
                                if sink.send(WsMessage::Text("{}".to_owned())).await.is_err() {
                                    return;
                                }
                            }
                        });
                    }
                    // Drain whatever the client sends (HELLO, pings) and never
                    // act on it. Dropping the read half instead would close the
                    // socket and hand the client the very disconnect these
                    // tests exist to prove it can find on its own.
                    while let Some(Ok(_)) = source.next().await {}
                });
            }
        });

        (format!("ws://{addr}"), accepted_rx)
    }

    /// A socket is reported connected only once the node signs our challenge.
    /// Each of these answers means the address is not that node, or is an
    /// older one, and the session must never come up.
    #[tokio::test]
    async fn no_session_without_a_valid_identity_proof() {
        for proof in [
            ProofBehaviour::WrongKey,
            ProofBehaviour::WrongChallenge,
            ProofBehaviour::None,
        ] {
            let (url, mut accepted) =
                serve_as(ServerBehaviour::Silent, node_identity(), proof).await;
            let (mut events, _reconnect, _send, task) = spawn_task(&url);
            assert!(accepted.recv().await.is_some(), "the node was dialed");
            // A refused session reports a disconnect and retries; it must
            // never report a connection.
            let deadline = tokio::time::Instant::now() + HELLO_PROOF_TIMEOUT * 3;
            let mut refused = false;
            while tokio::time::Instant::now() < deadline {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                match next_transition(&mut events, remaining).await.as_str() {
                    "connected" => panic!("an unproven socket was reported connected"),
                    "disconnected" => refused = true,
                    _ => break,
                }
            }
            assert!(refused, "the unproven session must end");
            task.abort();
        }
    }

    /// A message the node sends before its proof still arrives, after it.
    #[tokio::test]
    async fn messages_before_the_proof_are_delivered_after_it() {
        let node = node_identity();
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("ws://{}", listener.local_addr().expect("addr"));
        let server_node = Arc::clone(&node);
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let ws = accept_async(stream).await.expect("upgrade");
            let (mut sink, mut source) = ws.split();
            let Some(Ok(WsMessage::Text(hello))) = source.next().await else {
                return;
            };
            let early = SignalingMessage::new(MessageType::Pong, server_node.public_id());
            sink.send(WsMessage::Text(early.to_json().expect("json")))
                .await
                .expect("send early");
            let proof = proof_for(&hello, &server_node, ProofBehaviour::Valid).expect("proof");
            sink.send(WsMessage::Text(proof)).await.expect("send proof");
            while let Some(Ok(_)) = source.next().await {}
        });
        let (mut events, _reconnect, _send, task) = spawn_task(&url);
        let mut order = Vec::new();
        while order.len() < 2 {
            match tokio::time::timeout(Duration::from_secs(5), events.recv()).await {
                Ok(Some(InternalEvent::WsConnected { .. })) => order.push("connected"),
                Ok(Some(InternalEvent::WsSignalingMessage { msg, .. })) => {
                    assert_eq!(msg.msg_type, MessageType::Pong);
                    order.push("pong");
                }
                Ok(Some(_)) => {}
                other => panic!("expected the session, got {other:?}"),
            }
        }
        assert_eq!(order, ["connected", "pong"]);
        task.abort();
    }

    /// Spawn the task under test against `url`, returning its event stream and
    /// the handle that signals a network change.
    fn spawn_task(
        url: &str,
    ) -> (
        mpsc::Receiver<InternalEvent>,
        Arc<Notify>,
        mpsc::Sender<WsMessage>,
        tokio::task::JoinHandle<()>,
    ) {
        let (internal_tx, internal_rx) = mpsc::channel::<InternalEvent>(64);
        let (send_tx, send_rx) = mpsc::channel::<WsMessage>(16);
        let reconnect_now = Arc::new(Notify::new());
        let task = tokio::spawn(supernode_ws_task(
            Arc::new(Identity::generate()),
            None,
            node_identity().public_id(),
            Arc::new(Mutex::new(vec![url.to_owned()])),
            send_rx,
            internal_tx,
            Arc::clone(&reconnect_now),
        ));
        (internal_rx, reconnect_now, send_tx, task)
    }

    /// Await the next connect/disconnect transition, ignoring inbound traffic.
    async fn next_transition(rx: &mut mpsc::Receiver<InternalEvent>, within: Duration) -> String {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(remaining, rx.recv()).await {
                Err(_) => return "<nothing>".to_owned(),
                Ok(None) => return "<closed>".to_owned(),
                Ok(Some(InternalEvent::WsConnected { .. })) => return "connected".to_owned(),
                Ok(Some(InternalEvent::WsDisconnected { .. })) => return "disconnected".to_owned(),
                Ok(Some(_)) => continue,
            }
        }
    }

    /// The regression this whole change exists for.
    ///
    /// A socket whose network has gone away does not error and does not
    /// deliver. Before the read deadline the task parked in `ws_stream.next()`
    /// forever: no `WsDisconnected`, so the manager never tore down the relay,
    /// never re-dialed, and never told the UI anything was wrong. The client
    /// was simply offline until it was restarted.
    #[tokio::test]
    async fn a_link_that_goes_silent_is_dropped_and_redialed() {
        let (url, mut accepted) = serve(ServerBehaviour::Silent).await;
        let (mut events, _reconnect, _send, task) = spawn_task(&url);

        assert!(accepted.recv().await.is_some(), "server saw the first dial");
        assert_eq!(
            next_transition(&mut events, Duration::from_secs(5)).await,
            "connected"
        );

        // Nothing ever arrives on this socket, so only the deadline can save it.
        assert_eq!(
            next_transition(&mut events, READ_IDLE_TIMEOUT * 4).await,
            "disconnected",
            "the idle deadline must give up on a silent link"
        );

        // And having given up, it must actually come back.
        assert!(
            accepted.recv().await.is_some(),
            "the task must re-dial after abandoning a dead link"
        );
        assert_eq!(
            next_transition(&mut events, Duration::from_secs(5)).await,
            "connected"
        );

        task.abort();
    }

    /// The guard on the fix above: the deadline must not mistake a quiet
    /// supernode for a dead one. Any inbound frame proves the path carries
    /// traffic and defers the deadline.
    #[tokio::test]
    async fn inbound_traffic_keeps_a_quiet_link_alive() {
        let (url, mut accepted) = serve(ServerBehaviour::Chatty).await;
        let (mut events, _reconnect, _send, task) = spawn_task(&url);

        assert!(accepted.recv().await.is_some());
        assert_eq!(
            next_transition(&mut events, Duration::from_secs(5)).await,
            "connected"
        );

        // Several deadlines worth of time, with only the periodic frames from
        // the server holding it open.
        assert_eq!(
            next_transition(&mut events, READ_IDLE_TIMEOUT * 4).await,
            "<nothing>",
            "a link that keeps delivering must not be torn down"
        );

        task.abort();
    }

    /// The platform fast path. Without it recovery is bounded by the idle
    /// deadline, which is deliberately generous — a minute and a half offline
    /// after every walk out of Wi-Fi range.
    #[tokio::test]
    async fn a_network_change_redials_without_waiting_for_the_deadline() {
        let (url, mut accepted) = serve(ServerBehaviour::Silent).await;
        let (mut events, reconnect, _send, task) = spawn_task(&url);

        assert!(accepted.recv().await.is_some());
        assert_eq!(
            next_transition(&mut events, Duration::from_secs(5)).await,
            "connected"
        );

        let signalled = tokio::time::Instant::now();
        reconnect.notify_one();

        assert_eq!(
            next_transition(&mut events, Duration::from_secs(5)).await,
            "disconnected"
        );
        assert!(
            accepted.recv().await.is_some(),
            "the signal must produce a fresh dial"
        );
        assert!(
            signalled.elapsed() < READ_IDLE_TIMEOUT,
            "the re-dial must not wait out the idle deadline (took {:?})",
            signalled.elapsed()
        );

        task.abort();
    }

    #[test]
    fn an_established_session_resets_the_backoff() {
        // Repeated failures against an endpoint that never connects escalate.
        let (wait, next) = backoff_after_error(Duration::from_secs(8), false);
        assert_eq!(wait, Duration::from_secs(8));
        assert_eq!(next, Duration::from_secs(16));

        // But a session that actually ran proves the endpoint is fine, so the
        // next attempt starts over rather than inheriting a delay earned
        // against a network that no longer exists.
        let (wait, next) = backoff_after_error(Duration::from_secs(60), true);
        assert_eq!(wait, RECONNECT_BASE);
        assert_eq!(next, RECONNECT_BASE * 2);
    }

    #[test]
    fn backoff_is_capped() {
        let (wait, next) = backoff_after_error(RECONNECT_MAX, false);
        assert_eq!(wait, RECONNECT_MAX);
        assert_eq!(next, RECONNECT_MAX, "backoff must not grow past its cap");
    }
}
