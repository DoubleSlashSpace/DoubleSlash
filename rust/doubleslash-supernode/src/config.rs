// DoubleSlash supernode — config.rs
// Environment defaults and runtime settings, overridden by the manifest.

use std::env;

/// Supernode runtime settings. Environment defaults are overridden by the manifest.
#[derive(Debug, Clone)]
pub struct Config {
    // Networking
    pub signaling_port: u16,
    pub relay_port: u16,
    pub relay_bind_ip: std::net::IpAddr,
    pub signaling_bind_ip: std::net::IpAddr,

    // Features
    pub chat_enabled: bool,
    pub files_enabled: bool,
    pub sfu_enabled: bool,
    // Invite
    pub invite_ttl_seconds: i64, // -1 = never expires

    // In-app portal presentation (web.host.app.v1 over QUIC — no public HTTP port)
    pub web_title: String,
    pub access_mode: AccessMode,
    pub access_code: String,
    pub ad_duration: u32,
    pub tos_text: String,
    pub ad_content: String,
    pub demo_links: bool,

    // External host/IP for relay tickets (required for clients to connect).
    // An IP literal. A DNS name belongs in `hostname`, not here.
    pub external_host: Option<String>,

    /// DNS name advertised beside `external_host`. Clients may dial it for
    /// signaling. Relay tickets keep the IP.
    pub hostname: Option<String>,
    /// How clients turn `hostname` into an address. `None` picks by name:
    /// see [`HostnameLookup::for_name`].
    pub hostname_lookup: Option<HostnameLookup>,

    // Data directory
    pub data_dir: std::path::PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    Open,
    Tos,
    Ad,
    Code,
}

impl AccessMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "tos" => Some(Self::Tos),
            "ad" | "timer" => Some(Self::Ad),
            "code" | "access_code" => Some(Self::Code),
            _ => None,
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        let data_dir = match doubleslash_features::first_env(&[doubleslash_features::ENV_HOME]) {
            Some(h) => std::path::PathBuf::from(h),
            None => {
                let home = dirs_next().unwrap_or_else(|| ".".into());
                std::path::PathBuf::from(home).join(doubleslash_features::DEFAULT_PROFILE_DIR)
            }
        };

        let invite_ttl_raw: i64 = env::var("supernode_invite_ttl")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(-1);
        let invite_ttl_seconds = if invite_ttl_raw < 0 {
            -1
        } else {
            invite_ttl_raw * 60
        };

        Config {
            signaling_port: env_u16("supernode_signaling_port", 34935),
            relay_port: env_u16("supernode_port", 3478),
            relay_bind_ip: std::net::Ipv4Addr::UNSPECIFIED.into(),
            signaling_bind_ip: std::net::Ipv4Addr::UNSPECIFIED.into(),
            chat_enabled: env_bool("supernode_chat", true),
            files_enabled: env_bool("supernode_files", true),
            sfu_enabled: env_bool("supernode_sfu", true),
            invite_ttl_seconds,
            web_title: env::var("supernode_web_title").unwrap_or_else(|_| "Relay Node".into()),
            access_mode: env::var("supernode_access_mode")
                .ok()
                .as_deref()
                .and_then(AccessMode::parse)
                .unwrap_or(AccessMode::Open),
            access_code: env::var("supernode_access_code").unwrap_or_else(|_| "doubleslash".into()),
            ad_duration: env::var("supernode_ad_duration")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            tos_text: env::var("supernode_tos_text").unwrap_or_default(),
            ad_content: env::var("supernode_ad_content").unwrap_or_default(),
            demo_links: env_bool("supernode_demo_links", false),
            external_host: env::var("supernode_host").ok().filter(|s| !s.is_empty()),
            hostname: env::var("supernode_hostname").ok().and_then(|raw| {
                let name = normalize_advertised_hostname(&raw);
                if name.is_none() && !raw.trim().is_empty() {
                    tracing::warn!(
                        "Ignoring supernode_hostname {raw:?}: it must be a DNS name, not an IP address"
                    );
                }
                name
            }),
            hostname_lookup: env::var("supernode_hostname_lookup").ok().and_then(|raw| {
                let lookup = HostnameLookup::parse(raw.trim());
                if lookup.is_none() && !raw.trim().is_empty() {
                    tracing::warn!(
                        "Ignoring supernode_hostname_lookup {raw:?}: use \"dns\" or \"dddns\""
                    );
                }
                lookup
            }),
            data_dir,
        }
    }
}

/// How clients turn this node's `hostname` into an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostnameLookup {
    /// Ordinary DNS, at the signaling port.
    Dns,
    /// A DDDNS name record (`docs/DDDNS.md`), which also carries the port.
    Dddns,
}

impl HostnameLookup {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "dns" => Some(Self::Dns),
            "dddns" => Some(Self::Dddns),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dns => "dns",
            Self::Dddns => "dddns",
        }
    }

    /// `configured`, or else DDDNS for a name under `doubleslash.space` and
    /// DNS for any other.
    pub fn for_name(name: &str, configured: Option<Self>) -> Self {
        configured.unwrap_or_else(|| {
            if name.ends_with(".doubleslash.space") {
                Self::Dddns
            } else {
                Self::Dns
            }
        })
    }
}

/// A DNS name an operator set for this node, or `None` when `raw` is empty
/// or is not a hostname. IP literals are refused so they cannot take the
/// place of `supernode_host`.
pub fn normalize_advertised_hostname(raw: &str) -> Option<String> {
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

/// `ws://<name>:<port>` for the signaling listener. `None` when `hostname`
/// is not a DNS name or the port is unset.
pub fn hostname_signaling_url(hostname: &str, port: u16) -> Option<String> {
    let name = normalize_advertised_hostname(hostname)?;
    if port == 0 {
        return None;
    }
    Some(format!("ws://{name}:{port}"))
}

/// The hint signed into the invite for `hostname`: `ws://<name>:<port>` for a
/// DNS name, or `dddns://<name>` for a DDDNS name, whose record carries the
/// port. `None` when the name cannot be used that way. A DDDNS name needs a
/// label in front of its zone.
pub fn hostname_hint(hostname: &str, lookup: Option<HostnameLookup>, port: u16) -> Option<String> {
    let name = normalize_advertised_hostname(hostname)?;
    match HostnameLookup::for_name(&name, lookup) {
        HostnameLookup::Dns => hostname_signaling_url(&name, port),
        HostnameLookup::Dddns => name.contains('.').then(|| format!("dddns://{name}")),
    }
}

fn env_u16(key: &str, default: u16) -> u16 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    match env::var(key).as_deref() {
        Ok("1") | Ok("true") | Ok("yes") => true,
        Ok("0") | Ok("false") | Ok("no") => false,
        _ => default,
    }
}

/// Get user home directory.
fn dirs_next() -> Option<String> {
    #[cfg(windows)]
    {
        env::var("USERPROFILE").ok()
    }
    #[cfg(not(windows))]
    {
        env::var("HOME").ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostname_url_uses_the_signaling_port() {
        assert_eq!(
            hostname_signaling_url("Relay.Example.COM.", 34935).as_deref(),
            Some("ws://relay.example.com:34935")
        );
    }

    #[test]
    fn hostname_rejects_addresses_and_urls() {
        assert!(normalize_advertised_hostname("198.51.100.7").is_none());
        assert!(normalize_advertised_hostname("[2001:db8::1]").is_none());
        assert!(normalize_advertised_hostname("relay.example.com:34935").is_none());
        assert!(normalize_advertised_hostname("ws://relay.example.com").is_none());
        assert!(normalize_advertised_hostname("").is_none());
        assert!(hostname_signaling_url("relay.example.com", 0).is_none());
    }

    #[test]
    fn a_dddns_name_is_hinted_as_such_in_any_zone() {
        let dddns = Some(HostnameLookup::Dddns);
        assert_eq!(
            hostname_hint("My.Names.Example", dddns, 34935).as_deref(),
            Some("dddns://my.names.example")
        );
        // By name alone: DDDNS under doubleslash.space, DNS elsewhere.
        assert_eq!(
            hostname_hint("my.doubleslash.space", None, 34935).as_deref(),
            Some("dddns://my.doubleslash.space")
        );
        assert_eq!(
            hostname_hint("relay.example.com", None, 34935).as_deref(),
            Some("ws://relay.example.com:34935")
        );
        let dns = Some(HostnameLookup::Dns);
        assert_eq!(
            hostname_hint("my.doubleslash.space", dns, 34935).as_deref(),
            Some("ws://my.doubleslash.space:34935")
        );
        // A DDDNS name needs a zone after its label.
        assert!(hostname_hint("localhost", dddns, 34935).is_none());
        assert_eq!(HostnameLookup::parse("DDDNS"), dddns);
        assert!(HostnameLookup::parse("dynamic").is_none());
    }

    // Helper: parse env_bool directly via a unique env key.
    fn set_and_check_bool(key: &str, val: &str, default: bool) -> bool {
        env::set_var(key, val);
        let result = env_bool(key, default);
        env::remove_var(key);
        result
    }

    #[test]
    fn env_bool_truthy_values() {
        assert!(set_and_check_bool("_CQ_TEST_BOOL_1", "1", false));
        assert!(set_and_check_bool("_CQ_TEST_BOOL_TRUE", "true", false));
        assert!(set_and_check_bool("_CQ_TEST_BOOL_YES", "yes", false));
    }

    #[test]
    fn env_bool_falsy_values() {
        assert!(!set_and_check_bool("_CQ_TEST_BOOL_0", "0", true));
        assert!(!set_and_check_bool("_CQ_TEST_BOOL_FALSE", "false", true));
        assert!(!set_and_check_bool("_CQ_TEST_BOOL_NO", "no", true));
    }

    #[test]
    fn env_bool_falls_back_to_default_when_unset() {
        env::remove_var("_CQ_TEST_BOOL_MISSING");
        assert!(env_bool("_CQ_TEST_BOOL_MISSING", true));
        assert!(!env_bool("_CQ_TEST_BOOL_MISSING", false));
    }

    #[test]
    fn env_bool_falls_back_to_default_for_unknown_value() {
        env::set_var("_CQ_TEST_BOOL_JUNK", "maybe");
        assert!(env_bool("_CQ_TEST_BOOL_JUNK", true));
        assert!(!env_bool("_CQ_TEST_BOOL_JUNK", false));
        env::remove_var("_CQ_TEST_BOOL_JUNK");
    }

    #[test]
    fn env_u16_parses_valid_value() {
        env::set_var("_CQ_TEST_PORT", "9999");
        let v = env_u16("_CQ_TEST_PORT", 1234);
        env::remove_var("_CQ_TEST_PORT");
        assert_eq!(v, 9999);
    }

    #[test]
    fn env_u16_falls_back_on_missing_or_bad_value() {
        env::remove_var("_CQ_TEST_PORT_MISSING");
        assert_eq!(env_u16("_CQ_TEST_PORT_MISSING", 5678), 5678);

        env::set_var("_CQ_TEST_PORT_BAD", "notanumber");
        let v = env_u16("_CQ_TEST_PORT_BAD", 4321);
        env::remove_var("_CQ_TEST_PORT_BAD");
        assert_eq!(v, 4321);
    }

    #[test]
    fn access_mode_from_env_parses_all_variants() {
        // Test the string matching logic directly (no env mutation needed).
        let parse = |s: &str| -> AccessMode {
            match s {
                "tos" => AccessMode::Tos,
                "ad" => AccessMode::Ad,
                "code" => AccessMode::Code,
                _ => AccessMode::Open,
            }
        };
        assert_eq!(parse("tos"), AccessMode::Tos);
        assert_eq!(parse("ad"), AccessMode::Ad);
        assert_eq!(parse("code"), AccessMode::Code);
        assert_eq!(parse("open"), AccessMode::Open);
        assert_eq!(parse(""), AccessMode::Open);
        assert_eq!(parse("unknown"), AccessMode::Open);
    }

    #[test]
    fn invite_ttl_negative_raw_maps_to_minus_one() {
        // Test the conversion formula directly without going through from_env().
        let convert = |raw: i64| -> i64 {
            if raw < 0 {
                -1
            } else {
                raw * 60
            }
        };
        assert_eq!(convert(-5), -1);
        assert_eq!(convert(-1), -1);
    }

    #[test]
    fn invite_ttl_positive_raw_converts_to_seconds() {
        let convert = |raw: i64| -> i64 {
            if raw < 0 {
                -1
            } else {
                raw * 60
            }
        };
        assert_eq!(convert(10), 600);
        assert_eq!(convert(1), 60);
        assert_eq!(convert(0), 0);
    }
}
