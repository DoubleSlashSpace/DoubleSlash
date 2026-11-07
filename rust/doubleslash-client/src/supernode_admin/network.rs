//! Read-only address discovery on a managed server, independent of DNS/DDDNS.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use serde::Serialize;
use snm_transport::{SshTransport, Transport};

// `route get` performs a local route lookup; it sends no packets to these IPs.
// A missing route is a successful check with no address for that family.
const ADDRESS_PROBE: &str = "command -v ip >/dev/null 2>&1 || exit 127\n\
    ip -o -4 route get 1.1.1.1 2>/dev/null || true\n\
    ip -o -6 route get 2606:4700:4700::1111 2>/dev/null || true";

// Managed targets are Linux. This namespace-local proc file has entries
// when IPv6 is enabled, including on a host with only a loopback IPv6 address.
const BIND_PROBE: &str = "if test -r /proc/net/if_inet6 && read -r ipv6_entry < /proc/net/if_inet6 && test -n \"$ipv6_entry\"; then printf '::'; else printf '0.0.0.0'; fi";

pub(super) async fn probe_listen_bind(transport: &SshTransport) -> Result<String, String> {
    let output = tokio::time::timeout(Duration::from_secs(15), transport.run(BIND_PROBE))
        .await
        .map_err(|_| "The IPv6 availability check timed out".to_string())?
        .map_err(|e| format!("Could not check server IPv6 support: {e}"))?;
    if output.exit_code == 0 && matches!(output.stdout.trim(), "::" | "0.0.0.0") {
        Ok(output.stdout.trim().to_owned())
    } else {
        Err("The server did not return its IPv6 availability".into())
    }
}

#[derive(Default, Serialize)]
pub(super) struct PublicAddresses {
    pub ipv4: Option<Ipv4Addr>,
    pub ipv6: Option<Ipv6Addr>,
}

impl PublicAddresses {
    pub fn configured(ip: Option<IpAddr>) -> Self {
        match ip {
            Some(IpAddr::V4(ip)) => Self {
                ipv4: Some(ip),
                ipv6: None,
            },
            Some(IpAddr::V6(ip)) => Self {
                ipv4: None,
                ipv6: Some(ip),
            },
            None => Self::default(),
        }
    }

    fn add_route_sources(&mut self, stdout: &str) {
        for line in stdout.lines() {
            let mut words = line.split_whitespace();
            let Some(source) = words.find(|word| *word == "src").and_then(|_| words.next()) else {
                continue;
            };
            match source.parse::<IpAddr>() {
                Ok(IpAddr::V4(ip)) if public_route_source(IpAddr::V4(ip)) => {
                    self.ipv4 = Some(ip);
                }
                Ok(IpAddr::V6(ip)) if public_route_source(IpAddr::V6(ip)) => {
                    self.ipv6 = Some(ip);
                }
                _ => {}
            }
        }
    }
}

// Interface scope alone includes RFC1918 and IPv6 ULA addresses. Those must
// never be offered as the public address to enter in DDDNS.
fn public_route_source(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_multicast()
                || a == 0
                || a >= 240
                || (a == 100 && (64..128).contains(&b))
                || (a == 192 && b == 0 && (c == 0 || c == 2))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let [a, b, ..] = ip.segments();
            (a & 0xe000) == 0x2000 && !(a == 0x2001 && b == 0x0db8)
        }
    }
}

pub(super) async fn probe_addresses(
    transport: &SshTransport,
    configured: Option<IpAddr>,
) -> Result<PublicAddresses, String> {
    let output = tokio::time::timeout(Duration::from_secs(15), transport.run(ADDRESS_PROBE))
        .await
        .map_err(|_| "The server address check timed out".to_string())?
        .map_err(|e| format!("Could not check server addresses: {e}"))?;
    if output.exit_code != 0 {
        return Err("Could not check server addresses: the server needs the ip command".into());
    }
    let mut addresses = PublicAddresses::configured(configured);
    addresses.add_route_sources(&output.stdout);
    Ok(addresses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_keeps_configured_nat_address_and_fills_the_other_family() {
        let mut addresses =
            PublicAddresses::configured(Some(IpAddr::V4(Ipv4Addr::new(8, 8, 4, 4))));
        addresses.add_route_sources(
            "1.1.1.1 via 10.0.0.1 dev eth0 src 10.0.0.2 uid 0\n\
            2606:4700:4700::1111 dev eth0 src 2606:4700::1234 metric 100\n",
        );
        assert_eq!(addresses.ipv4, Some(Ipv4Addr::new(8, 8, 4, 4)));
        assert_eq!(
            addresses.ipv6.map(|ip| ip.to_string()).as_deref(),
            Some("2606:4700::1234")
        );
        // A newly observed public route source replaces an outdated saved IP.
        addresses.add_route_sources("1.1.1.1 dev eth0 src 9.9.9.9\n");
        assert_eq!(addresses.ipv4, Some(Ipv4Addr::new(9, 9, 9, 9)));
        assert!(addresses.ipv6.is_some());
    }

    #[test]
    fn private_special_and_malformed_sources_are_not_public_addresses() {
        for source in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.0.1",
            "100.64.0.1",
            "169.254.1.1",
            "0.0.0.0",
            "198.18.0.1",
            "192.0.2.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "fe80::1",
            "fc00::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:192.168.0.1",
            "bad",
        ] {
            let mut addresses = PublicAddresses::default();
            addresses.add_route_sources(&format!("destination dev eth0 src {source}"));
            assert!(
                addresses.ipv4.is_none() && addresses.ipv6.is_none(),
                "{source}"
            );
        }
    }
}
