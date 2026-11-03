//! Connection fallback logic — pure-data, no I/O.
//!
//! ## Candidate ordering ([`build_ws_candidates`])
//!
//! Returns an ordered, de-duplicated list of WebSocket connect candidates:
//! 1. Connected / known supernode relay endpoints (fastest when both peers
//!    are behind NAT and a trusted supernode is already reachable).
//! 2. Direct invite/store endpoint.
//! 3. LAN hint advertised by the peer.
//! 4. Any further relay hints from the peer record.
//!
//! ## Direct-call fallback ([`DirectFallbackCoordinator`])
//!
//! When a direct QUIC call cannot be established the client falls back to a
//! temporary private SFU room on a trusted supernode. The connection manager
//! owns the I/O; this module holds the pure state machine and helpers.

use std::collections::HashSet;
use std::net::IpAddr;

// ---------------------------------------------------------------------------
// Candidate ordering
// ---------------------------------------------------------------------------

/// Prefix used for temporary direct-fallback rooms.
pub const TEMP_ROOM_PREFIX: &str = "direct-";

/// Build an ordered, de-duplicated list of WebSocket connect candidates.
///
/// # Arguments
///
/// * `endpoint_url`                  – peer's primary direct endpoint.
/// * `lan_hint`                      – best-effort LAN address from the peer.
/// * `supernode_relay_hints`         – caller-supplied relay hints.
/// * `connected_supernode_endpoints` – WS URLs of currently-connected trusted supernodes.
/// * `peer_relay_hints`              – further `relay_hints` from the peer record.
pub fn build_ws_candidates<'a>(
    endpoint_url: Option<&'a str>,
    lan_hint: Option<&'a str>,
    supernode_relay_hints: impl IntoIterator<Item = &'a str>,
    connected_supernode_endpoints: impl IntoIterator<Item = &'a str>,
    peer_relay_hints: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut candidates: Vec<String> = Vec::new();

    let mut push = |url: &str| {
        if !url.is_empty() && seen.insert(url.to_owned()) {
            candidates.push(url.to_owned());
        }
    };

    for h in connected_supernode_endpoints {
        push(h);
    }
    for h in supernode_relay_hints {
        push(h);
    }
    if let Some(ep) = endpoint_url {
        push(ep);
    }
    if let Some(lh) = lan_hint {
        push(lh);
    }
    for h in peer_relay_hints {
        push(h);
    }

    candidates
}

/// Convenience: ordered, de-duplicated list from a peer/supernode's stored
/// `relay_hints` (and optional primary endpoint).
pub fn build_ws_candidates_from_hints(
    endpoint_url: Option<&str>,
    relay_hints: &[String],
) -> Vec<String> {
    build_ws_candidates(
        endpoint_url,
        None,
        std::iter::empty(),
        std::iter::empty(),
        relay_hints.iter().map(String::as_str),
    )
}

/// A DNS name an operator advertised for a supernode. IP literals, URLs,
/// and empty strings are refused: the IP already travels separately.
pub fn normalize_dns_name(raw: &str) -> Option<String> {
    let name = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty() || name.len() > 253 || name.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    if name.contains(':') || name.contains('/') || name.contains('@') {
        return None;
    }
    let ok = name.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    });
    ok.then_some(name)
}

/// Parent zone whose names are DDDNS records, not DNS addresses of a supernode.
pub const DDDNS_PARENT: &str = "doubleslash.space";

/// `label.doubleslash.space`: exactly one label in the zone this client knows
/// is DDDNS (see `docs/DDDNS.md`).
///
/// Ordinary DNS for any of these names answers with the DDDNS servers, so a
/// caller must look the name up over HTTPS instead of resolving it. A deeper
/// name such as `a.b.doubleslash.space` is not a DDDNS name.
pub fn dddns_name(hostname: &str) -> Option<String> {
    let name = normalize_dns_name(hostname)?;
    let label = name.strip_suffix(&format!(".{DDDNS_PARENT}"))?;
    if label.is_empty() || label.contains('.') {
        return None;
    }
    Some(name)
}

/// Whether `hostname` can be looked up as a DDDNS name at all: a valid DNS
/// name with at least one label before its zone.
pub fn is_dddns_lookup_name(hostname: &str) -> bool {
    normalize_dns_name(hostname).is_some_and(|name| dddns_label(&name).is_some())
}

/// Scheme of a hint that marks a hostname as a DDDNS name in any zone.
pub const DDDNS_HINT_SCHEME: &str = "dddns";

/// `dddns://<name>`: the hint a supernode signs into its invite, and names in
/// `SupernodeInfo`, when its hostname is a DDDNS name. It carries no port; the
/// record does. `None` when `hostname` is not a name in a zone.
pub fn dddns_hint(hostname: &str) -> Option<String> {
    let name = normalize_dns_name(hostname)?;
    dddns_label(&name)?;
    Some(format!("{DDDNS_HINT_SCHEME}://{name}"))
}

/// The name in a `dddns://` hint, or `None` when `url` is not one. A port,
/// path or anything else after the name makes the hint unusable.
fn dddns_hint_name(url: &str) -> Option<Option<String>> {
    let (scheme, rest) = url.trim().split_once("://")?;
    if !scheme.eq_ignore_ascii_case(DDDNS_HINT_SCHEME) {
        return None;
    }
    let name = rest.strip_suffix('/').unwrap_or(rest);
    Some(dddns_hint(name).and_then(|_| normalize_dns_name(name)))
}

/// The label a DDDNS record for `name` must carry: its first label, when the
/// name has a zone after it. A DDDNS service in any zone answers this way, so
/// a name outside [`DDDNS_PARENT`] can be looked up too.
fn dddns_label(name: &str) -> Option<&str> {
    let (label, zone) = name.split_once('.')?;
    (!label.is_empty() && !zone.is_empty()).then_some(label)
}

/// How the WebSocket task should turn a stored hint into an IP dial target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalingHint {
    /// Dial this URL as given. Its host is already an IP, or it is not a
    /// hostname this client knows how to resolve.
    Direct,
    /// Resolve `name` with DNS and dial `scheme://ip:port`.
    Dns {
        name: String,
        scheme: &'static str,
        port: u16,
    },
    /// `GET https://name/.well-known/doubleslash`. The record carries the IP
    /// and the signaling port.
    Dddns { name: String, scheme: &'static str },
    /// A `dddns://` hint whose name cannot be looked up. Nothing to dial.
    Unusable,
}

/// Classify a stored signaling URL.
///
/// `dddns://<name>` is a DDDNS name in any zone, dialed over `ws://` at the
/// record's port. A `ws://` or `wss://` URL whose host is a name under
/// [`DDDNS_PARENT`] is one too, so links minted before the `dddns://` hint
/// keep working. Neither is ever [`SignalingHint::Dns`]: DNS for a DDDNS name
/// answers with the service's own servers.
pub fn classify_signaling_hint(url: &str) -> SignalingHint {
    if let Some(name) = dddns_hint_name(url) {
        return match name {
            Some(name) => SignalingHint::Dddns { name, scheme: "ws" },
            None => SignalingHint::Unusable,
        };
    }
    let Some((scheme, host, port)) = ws_authority(url) else {
        return SignalingHint::Direct;
    };
    if host.parse::<IpAddr>().is_ok() {
        return SignalingHint::Direct;
    }
    let Some(name) = normalize_dns_name(&host) else {
        return SignalingHint::Direct;
    };
    if dddns_name(&name).is_some() {
        return SignalingHint::Dddns { name, scheme };
    }
    match port {
        Some(port) if port != 0 => SignalingHint::Dns { name, scheme, port },
        _ => SignalingHint::Direct,
    }
}

/// Addresses from a DDDNS name record (`v` 1, status `active` or `grace`).
/// `name` is the hostname that was looked up, in any zone. The record's `name`
/// is that hostname's first label, and it has to match.
pub fn dddns_endpoints_from_json(name: &str, body: &str) -> Result<Vec<(IpAddr, u16)>, String> {
    let fqdn = normalize_dns_name(name).ok_or_else(|| "not a DNS name".to_owned())?;
    let label = dddns_label(&fqdn).ok_or_else(|| "not a name in a zone".to_owned())?;
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|error| format!("record is not JSON: {error}"))?;
    let version = value.get("v").and_then(serde_json::Value::as_u64);
    if version != Some(1) {
        return Err(format!("unsupported record version {version:?}"));
    }
    let got = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if got != label {
        return Err(format!("record is for {got}, not {label}"));
    }
    let status = value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if status != "active" && status != "grace" {
        return Err(format!("name is {status}"));
    }
    let port = value
        .get("port")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if port == 0 || port > u64::from(u16::MAX) {
        return Err("record has no signaling port".into());
    }
    let port = port as u16;
    let mut out = Vec::new();
    if let Some(text) = value.get("ipv4").and_then(serde_json::Value::as_str) {
        if let Ok(IpAddr::V4(ip)) = text.parse() {
            push_dialable(&mut out, IpAddr::V4(ip), port);
        }
    }
    if let Some(text) = value.get("ipv6").and_then(serde_json::Value::as_str) {
        if let Ok(IpAddr::V6(ip)) = text.parse() {
            push_dialable(&mut out, IpAddr::V6(ip), port);
        }
    }
    if out.is_empty() {
        return Err("record has no address".into());
    }
    Ok(out)
}

fn push_dialable(out: &mut Vec<(IpAddr, u16)>, ip: IpAddr, port: u16) {
    if ip.is_unspecified() || ip.is_multicast() {
        return;
    }
    if !out.iter().any(|(existing, _)| existing == &ip) {
        out.push((ip, port));
    }
}

/// `ws://` or `wss://` URL for an IP. IPv6 is bracketed.
pub fn signaling_url_for(scheme: &str, ip: IpAddr, port: u16) -> Option<String> {
    if port == 0 {
        return None;
    }
    let scheme = match scheme {
        "wss" => "wss",
        "ws" => "ws",
        _ => return None,
    };
    Some(match ip {
        IpAddr::V4(ip) => format!("{scheme}://{ip}:{port}"),
        IpAddr::V6(ip) => format!("{scheme}://[{ip}]:{port}"),
    })
}

fn ws_authority(url: &str) -> Option<(&'static str, String, Option<u16>)> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = if scheme.eq_ignore_ascii_case("wss") {
        "wss"
    } else if scheme.eq_ignore_ascii_case("ws") {
        "ws"
    } else {
        return None;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    if authority.is_empty() {
        return None;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = after.strip_prefix(':').and_then(|text| text.parse().ok());
        (host.to_owned(), port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        (host.to_owned(), port.parse().ok())
    } else {
        (authority.to_owned(), None)
    };
    if host.is_empty() {
        return None;
    }
    Some((scheme, host, port))
}

/// Signaling URL for a supernode hostname, using the scheme and port of the
/// address we already reach that node on. This is stored beside the IP. It
/// is resolved to an address only after that IP connection fails.
pub fn hostname_ws_url(hostname: &str, scheme: &str, port: u16) -> Option<String> {
    let name = normalize_dns_name(hostname)?;
    if port == 0 {
        return None;
    }
    let scheme = match scheme {
        "wss" => "wss",
        "ws" => "ws",
        _ => return None,
    };
    Some(format!("{scheme}://{name}:{port}"))
}

/// Scheme and port of a `ws://` or `wss://` URL, including a bracketed IPv6 host.
pub fn signaling_endpoint(ws_url: &str) -> Option<(&'static str, u16)> {
    let (scheme, rest) = ws_url.split_once("://")?;
    let scheme = if scheme.eq_ignore_ascii_case("wss") {
        "wss"
    } else if scheme.eq_ignore_ascii_case("ws") {
        "ws"
    } else {
        return None;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    if authority.is_empty() {
        return None;
    }
    let port_str = if let Some(host) = authority.strip_prefix('[') {
        let (_, after) = host.split_once(']')?;
        after.strip_prefix(':')?
    } else {
        authority.rsplit_once(':')?.1
    };
    let port = port_str.parse().ok()?;
    (port != 0).then_some((scheme, port))
}

/// Dial list for a supernode invite: the IP hint first, then any extra
/// `ws://` / `wss://` hints. The IP is what the client dials. A hostname in
/// the list is resolved only after that connection fails. Duplicates drop out.
pub fn ws_hints_from_invite(primary: &str, extra: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |url: &str| {
        let url = url.trim();
        if url.is_empty() || out.iter().any(|existing| existing == url) {
            return;
        }
        if url.starts_with("ws://") || url.starts_with("wss://") || dddns_hint_name(url).is_some() {
            out.push(url.to_owned());
        }
    };
    push(primary);
    for url in extra {
        push(url);
    }
    out
}

// ---------------------------------------------------------------------------
// DirectFallbackCoordinator
// ---------------------------------------------------------------------------

/// State for a pending direct-call → private-room fallback.
#[derive(Debug, Clone)]
pub struct PendingFallback {
    /// The original target peer.
    pub peer_id: String,
    /// The temporary room id that was created.
    pub room_id: String,
    /// The supernode that is hosting the room.
    pub supernode_id: String,
}

/// State machine for direct-call → private-room fallback.
///
/// Holds at most one pending fallback at a time.
#[derive(Debug, Default)]
pub struct DirectFallbackCoordinator {
    pending: Option<PendingFallback>,
    /// Monotonic counter embedded in room ids so concurrent profiles don't
    /// collide when identity prefixes match.
    counter: u32,
}

impl DirectFallbackCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    /// The currently pending fallback, if any.
    pub fn pending(&self) -> Option<&PendingFallback> {
        self.pending.as_ref()
    }

    /// Take the next room-id counter value.
    pub fn next_counter(&mut self) -> u32 {
        self.counter = self.counter.wrapping_add(1);
        self.counter
    }

    /// Schedule a fallback for the given peer.
    pub fn set_pending(&mut self, pending: PendingFallback) {
        self.pending = Some(pending);
    }

    /// Drop any pending fallback (direct call recovered or session stopped).
    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// Returns `true` if a fallback is pending for `peer_id`.
    pub fn is_pending_for(&self, peer_id: &str) -> bool {
        self.pending
            .as_ref()
            .map(|p| p.peer_id == peer_id)
            .unwrap_or(false)
    }

    /// Returns `true` if the given room id is the currently pending fallback room.
    pub fn is_pending_room(&self, supernode_id: &str, room_id: &str) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| p.supernode_id == supernode_id && p.room_id == room_id)
    }

    /// Return `true` for room IDs minted by the fallback path.
    pub fn is_temp_direct_room(room_id: &str) -> bool {
        room_id.starts_with(TEMP_ROOM_PREFIX)
    }

    /// Build a deterministic temporary room id for a direct-call fallback.
    ///
    /// Embeds short prefixes of both identity public keys plus a counter so
    /// concurrent fallbacks across profiles don't collide.
    pub fn build_room_id(self_pub: &str, peer_id: &str, counter: u32) -> String {
        let sp = &self_pub[..12.min(self_pub.len())];
        let pp = &peer_id[..12.min(peer_id.len())];
        format!("{TEMP_ROOM_PREFIX}{sp}-{pp}-{counter}")
    }

    /// Pick a trusted supernode id for fallback.
    ///
    /// Preference order:
    /// 1. A trusted supernode that is currently connected via WebSocket.
    /// 2. Any trusted supernode in the provided list.
    /// 3. Empty string when none qualify.
    pub fn pick_supernode<'a>(
        supernode_ids: impl IntoIterator<Item = &'a str>,
        connected_ids: &HashSet<String>,
    ) -> String {
        let mut fallback: Option<String> = None;
        for id in supernode_ids {
            if connected_ids.contains(id) {
                return id.to_owned();
            }
            if fallback.is_none() {
                fallback = Some(id.to_owned());
            }
        }
        fallback.unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_hints_keep_the_ip_ahead_of_the_hostname() {
        let hints = ws_hints_from_invite(
            "ws://198.51.100.7:34935",
            &[
                "ws://relay.example.com:34935".into(),
                "ws://198.51.100.7:34935".into(),
                "turn:198.51.100.7:3478".into(),
                "not a url".into(),
            ],
        );
        assert_eq!(
            hints,
            vec![
                "ws://198.51.100.7:34935".to_owned(),
                "ws://relay.example.com:34935".to_owned(),
            ]
        );
    }

    #[test]
    fn hostname_url_uses_the_live_scheme_and_port() {
        assert_eq!(
            signaling_endpoint("ws://198.51.100.7:34935"),
            Some(("ws", 34935))
        );
        assert_eq!(
            signaling_endpoint("wss://[2001:db8::7]:34935/sig"),
            Some(("wss", 34935))
        );
        assert_eq!(
            hostname_ws_url("Relay.Example.COM.", "wss", 34935).as_deref(),
            Some("wss://relay.example.com:34935")
        );
        assert!(hostname_ws_url("198.51.100.7", "ws", 34935).is_none());
    }

    #[test]
    fn doubleslash_space_is_a_dddns_name_and_not_a_dns_lookup() {
        assert_eq!(
            dddns_name("My.DoubleSlash.Space."),
            Some("my.doubleslash.space".into())
        );
        assert!(dddns_name("relay.example.com").is_none());
        assert!(dddns_name("doubleslash.space").is_none());
        assert!(dddns_name("a.b.doubleslash.space").is_none());
        assert!(is_dddns_lookup_name("relay.example.com"));
        assert!(!is_dddns_lookup_name("localhost"));
        assert!(!is_dddns_lookup_name("198.51.100.7"));
    }

    #[test]
    fn a_dddns_hint_names_a_dddns_service_in_any_zone() {
        assert_eq!(
            dddns_hint("MyServer.Names.Example.").as_deref(),
            Some("dddns://myserver.names.example")
        );
        assert!(dddns_hint("localhost").is_none());
        assert!(dddns_hint("198.51.100.7").is_none());
        assert_eq!(
            classify_signaling_hint("dddns://myserver.names.example"),
            SignalingHint::Dddns {
                name: "myserver.names.example".into(),
                scheme: "ws",
            }
        );
        assert_eq!(
            classify_signaling_hint("DDDNS://myserver.doubleslash.space/"),
            SignalingHint::Dddns {
                name: "myserver.doubleslash.space".into(),
                scheme: "ws",
            }
        );
        // A port, a path or a bare word is not a usable hint.
        for bad in [
            "dddns://myserver.names.example:34935",
            "dddns://myserver.names.example/x",
            "dddns://localhost",
            "dddns://",
        ] {
            assert_eq!(
                classify_signaling_hint(bad),
                SignalingHint::Unusable,
                "{bad}"
            );
        }
        // Without the hint, a name in another zone is ordinary DNS.
        assert!(matches!(
            classify_signaling_hint("ws://myserver.names.example:34935"),
            SignalingHint::Dns { .. }
        ));
        // A signed invite's DDDNS hint survives into the dial list.
        assert_eq!(
            ws_hints_from_invite(
                "ws://198.51.100.7:34935",
                &["dddns://myserver.names.example".into(), "https://x".into()],
            ),
            vec![
                "ws://198.51.100.7:34935".to_owned(),
                "dddns://myserver.names.example".to_owned(),
            ]
        );
        assert_eq!(
            classify_signaling_hint("ws://my.doubleslash.space:34935"),
            SignalingHint::Dddns {
                name: "my.doubleslash.space".into(),
                scheme: "ws",
            }
        );
        // Not a DDDNS name: two labels in front of the zone.
        assert_eq!(
            classify_signaling_hint("wss://a.b.doubleslash.space:34935"),
            SignalingHint::Dns {
                name: "a.b.doubleslash.space".into(),
                scheme: "wss",
                port: 34935,
            }
        );
        assert_eq!(
            classify_signaling_hint("ws://relay.example.com:34935"),
            SignalingHint::Dns {
                name: "relay.example.com".into(),
                scheme: "ws",
                port: 34935,
            }
        );
        assert_eq!(
            classify_signaling_hint("ws://198.51.100.7:34935"),
            SignalingHint::Direct
        );
        assert_eq!(
            classify_signaling_hint("ws://[2001:db8::7]:34935"),
            SignalingHint::Direct
        );
    }

    #[test]
    fn dddns_record_yields_the_ip_and_port() {
        let body = r#"{
            "v": 1,
            "name": "myfortniteserver",
            "status": "active",
            "ipv4": "203.0.113.7",
            "ipv6": "2001:db8::7",
            "port": 35035
        }"#;
        let endpoints =
            dddns_endpoints_from_json("myfortniteserver.doubleslash.space", body).expect("record");
        assert_eq!(
            endpoints,
            vec![
                ("203.0.113.7".parse().unwrap(), 35035),
                ("2001:db8::7".parse().unwrap(), 35035),
            ]
        );
        assert_eq!(
            signaling_url_for("ws", endpoints[1].0, endpoints[1].1).as_deref(),
            Some("ws://[2001:db8::7]:35035")
        );
        let grace = body.replace("active", "grace");
        assert!(dddns_endpoints_from_json("myfortniteserver.doubleslash.space", &grace).is_ok());
        let available = body.replace("active", "available");
        assert!(
            dddns_endpoints_from_json("myfortniteserver.doubleslash.space", &available).is_err()
        );
        assert!(dddns_endpoints_from_json("other.doubleslash.space", body).is_err());
        // The same record served by a DDDNS service in another zone.
        assert!(dddns_endpoints_from_json("myfortniteserver.names.example", body).is_ok());
        assert!(dddns_endpoints_from_json("myfortniteserver", body).is_err());
        let next = body.replace("\"v\": 1", "\"v\": 2");
        assert!(dddns_endpoints_from_json("myfortniteserver.doubleslash.space", &next).is_err());
    }

    #[test]
    fn candidate_order_supernode_first() {
        let candidates = build_ws_candidates(
            Some("ws://direct:8080"),
            Some("ws://192.168.1.2:8080"),
            std::iter::empty(),
            ["ws://supernode:443"],
            std::iter::empty(),
        );
        assert_eq!(candidates[0], "ws://supernode:443");
        assert_eq!(candidates[1], "ws://direct:8080");
        assert_eq!(candidates[2], "ws://192.168.1.2:8080");
    }

    #[test]
    fn candidate_order_all_tiers() {
        let candidates = build_ws_candidates(
            Some("ws://direct:4000"),
            Some("ws://lan:4000"),
            ["ws://relay-hint:4001"],
            ["ws://supernode:4002"],
            ["ws://peer-relay:4003"],
        );
        assert_eq!(
            candidates,
            vec![
                "ws://supernode:4002".to_owned(),  // connected supernodes first
                "ws://relay-hint:4001".to_owned(), // then supernode relay hints
                "ws://direct:4000".to_owned(),     // then direct endpoint
                "ws://lan:4000".to_owned(),        // then LAN hint
                "ws://peer-relay:4003".to_owned(), // peer relay hints last
            ]
        );
    }

    #[test]
    fn candidates_skip_empty() {
        let candidates =
            build_ws_candidates(Some(""), None, std::iter::empty(), [""], std::iter::empty());
        assert!(candidates.is_empty());
    }

    #[test]
    fn candidate_dedup() {
        let candidates = build_ws_candidates(
            Some("ws://node:9000"),
            None,
            ["ws://node:9000"],
            ["ws://node:9000"],
            std::iter::empty(),
        );
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn from_hints_preserves_order_and_dedups() {
        let hints = vec![
            "ws://a:1".to_owned(),
            "ws://b:2".to_owned(),
            "ws://a:1".to_owned(),
        ];
        let c = build_ws_candidates_from_hints(Some("ws://primary:9"), &hints);
        assert_eq!(
            c,
            vec![
                "ws://primary:9".to_owned(),
                "ws://a:1".to_owned(),
                "ws://b:2".to_owned(),
            ]
        );
    }

    #[test]
    fn build_room_id_format() {
        let id =
            DirectFallbackCoordinator::build_room_id("aabbccddeeff0011", "112233445566ffee", 7);
        assert!(id.starts_with("direct-"));
        assert!(DirectFallbackCoordinator::is_temp_direct_room(&id));
    }

    #[test]
    fn pick_supernode_prefers_connected() {
        let mut connected = HashSet::new();
        connected.insert("sn2".to_owned());
        let picked = DirectFallbackCoordinator::pick_supernode(["sn1", "sn2", "sn3"], &connected);
        assert_eq!(picked, "sn2");
    }

    #[test]
    fn pick_supernode_falls_back_to_first() {
        let connected = HashSet::new();
        let picked = DirectFallbackCoordinator::pick_supernode(["sn1", "sn2"], &connected);
        assert_eq!(picked, "sn1");
    }

    #[test]
    fn pending_room_match() {
        let mut c = DirectFallbackCoordinator::new();
        c.set_pending(PendingFallback {
            peer_id: "p".into(),
            room_id: "direct-x".into(),
            supernode_id: "sn".into(),
        });
        assert!(c.is_pending_room("sn", "direct-x"));
        assert!(!c.is_pending_room("sn", "other"));
        c.cancel();
        assert!(c.pending().is_none());
    }
}
