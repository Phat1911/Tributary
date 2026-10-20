//! Network helpers shared by both planes: client-IP extraction (edge-proxy
//! trust model) and CIDR matching for per-key IP allowlists (SPEC §9, M4).
//!
//! Deliberately dependency-free — `std::net` already parses IP literals,
//! and CIDR containment is one mask compare. No GeoIP anything lives here
//! (the impossible-travel resolver is a dormant feature; see the gateway's
//! `geo` module).

use std::net::IpAddr;

/// The header-map type is `http`'s (axum re-exports it as `axum::http`) —
/// pulling the small crate keeps `common` free of a web-framework dep.
use http::HeaderMap;

/// Best-effort client IP: the FIRST `X-Forwarded-For` hop (written by the
/// deployment's reverse proxy, SPEC §8 TLS edge) or `"local"` for direct
/// un-proxied connections.
///
/// Trust model: only the edge proxy writes XFF in a sane deployment; a
/// client-forged XFF can claim a fake IP, which is exactly why the
/// allowlist decision ALSO treats unparseable values as non-matching
/// (fail-closed) rather than trusting blindly.
pub fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|ip| !ip.is_empty())
        .unwrap_or("local")
        .to_string()
}

/// The "shared network" an address belongs to, for concurrency grouping
/// (D32): IPv4 collapses to its /24 (the classic ISP/office block), IPv6
/// to its /64 (the standard customer site delegation). Two addresses in
/// the same shared network are ONE source as far as key-sharing detection
/// is concerned — a laptop hopping .9 → .11 inside one /24 is not news.
pub fn shared_network_of(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            IpAddr::V4(std::net::Ipv4Addr::new(o[0], o[1], o[2], 0))
        }
        IpAddr::V6(v6) => {
            let o = v6.octets();
            let mut collapsed = [0u8; 16];
            collapsed[..8].copy_from_slice(&o[..8]);
            IpAddr::V6(std::net::Ipv6Addr::from(collapsed))
        }
    }
}

/// Convenience: parse a client-IP string to its shared network. `None`
/// covers "local", garbage, or empty values — callers skip those sources
/// rather than guess (the fail-closed house style).
pub fn parse_shared_network(raw: &str) -> Option<IpAddr> {
    raw.parse::<IpAddr>().ok().map(shared_network_of)
}

/// One allowlist entry: an IP network. A bare IP means a /32 (v4) or /128
/// (v6) network — admins shouldn't have to type the suffix for a single
/// host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cidr {
    network: IpAddr,
    prefix_len: u8,
}

impl Cidr {
    /// Parses `a.b.c.d/n`, `a.b.c.d` (→ /32), or the IPv6 equivalents.
    /// Returns `None` on anything malformed — callers fail-closed.
    pub fn parse(spec: &str) -> Option<Self> {
        let spec = spec.trim();
        let (addr, prefix) = match spec.split_once('/') {
            Some((addr, prefix)) => (addr, prefix.parse::<u8>().ok()?),
            None => (spec, u8::MAX), // sentinel: derive from the parsed family
        };
        let network: IpAddr = addr.parse().ok()?;
        let max = match network {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        // A bare IP gets the full-length prefix; an explicit one must fit
        // its family (`/40` on IPv4 is a typo, not a network).
        let prefix_len = if prefix == u8::MAX { max } else { prefix };
        if prefix_len > max {
            return None;
        }
        Some(Self {
            network,
            prefix_len,
        })
    }

    /// True when `ip` falls inside this network.
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.network, ip) {
            (IpAddr::V4(net), IpAddr::V4(addr)) => {
                let len = u32::from(self.prefix_len);
                if len == 0 {
                    return true; // 0.0.0.0/0 matches everything v4
                }
                let mask = u32::MAX << (32 - len);
                (u32::from(net) & mask) == (u32::from(addr) & mask)
            }
            (IpAddr::V6(net), IpAddr::V6(addr)) => {
                let len = u128::from(self.prefix_len);
                if len == 0 {
                    return true;
                }
                let mask = u128::MAX << (128 - len);
                (u128::from(net) & mask) == (u128::from(addr) & mask)
            }
            // Mixed families never match (no NAT translation semantics here).
            _ => false,
        }
    }
}

/// The allowlist verdict for one request (fail-closed):
/// - `NoRules` — the key has no allowlist rows; any source is fine
///   (dormant-by-default, SPEC §9).
/// - `Matches` — at least one configured CIDR contains the source IP.
/// - `Denied` — rules exist but nothing matches (including unparseable or
///   `"local"` sources — a proxy-less direct connection cannot prove it is
///   inside the office network).
pub enum AllowlistVerdict {
    NoRules,
    Matches,
    Denied,
}

/// Evaluates a source IP against a key's CIDR set (fail-closed).
pub fn evaluate_allowlist(cidrs: &[Cidr], source_ip: &str) -> AllowlistVerdict {
    if cidrs.is_empty() {
        return AllowlistVerdict::NoRules;
    }
    // "local" (no XFF, direct connection) and garbage strings match nothing.
    let Ok(ip) = source_ip.parse::<IpAddr>() else {
        return AllowlistVerdict::Denied;
    };
    if cidrs.iter().any(|c| c.contains(ip)) {
        AllowlistVerdict::Matches
    } else {
        AllowlistVerdict::Denied
    }
}

/// Renders a canonical display form (`network/len`) for listings.
pub fn format_cidr(cidr: &Cidr) -> String {
    format!("{}/{}", cidr.network, cidr.prefix_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses an XFF header value the way the edge proxy writes it.
    fn ip_of(xff: &str) -> String {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", xff.parse().unwrap());
        client_ip(&headers)
    }

    #[test]
    fn client_ip_prefers_first_xff_hop() {
        assert_eq!(ip_of("9.9.9.9, 10.0.0.1"), "9.9.9.9");
        assert_eq!(client_ip(&HeaderMap::new()), "local");
    }

    #[test]
    fn cidr_parses_v4_v6_and_bare_ips() {
        assert_eq!(Cidr::parse("10.0.0.0/8").unwrap().prefix_len, 8);
        assert_eq!(Cidr::parse("10.1.2.3").unwrap().prefix_len, 32);
        assert!(Cidr::parse("2001:db8::/32").is_some());
        assert!(Cidr::parse("2001:db8::1").is_some());
        // Malformed input never yields a rule (fail-closed parsing).
        assert!(Cidr::parse("10.0.0.0/33").is_none());
        assert!(Cidr::parse("nonsense").is_none());
        assert!(Cidr::parse("").is_none());
        assert!(Cidr::parse("10.0.0.0/-1").is_none());
    }

    #[test]
    fn shared_network_collapses_v4_to_24_and_v6_to_64() {
        use std::net::IpAddr;
        // Same /24 = one source, regardless of host part.
        assert_eq!(
            parse_shared_network("91.207.174.9"),
            Some("91.207.174.0".parse::<IpAddr>().unwrap())
        );
        assert_eq!(
            parse_shared_network("91.207.174.254"),
            Some("91.207.174.0".parse::<IpAddr>().unwrap())
        );
        // Different /24 = two sources.
        assert_ne!(
            parse_shared_network("91.207.174.9"),
            parse_shared_network("203.0.113.7")
        );
        // v6 collapses at /64 (customer site), not /128.
        assert_eq!(
            parse_shared_network("2001:db8:1:2:3:4:5:6"),
            parse_shared_network("2001:db8:1:2:ffff::1")
        );
        assert_ne!(
            parse_shared_network("2001:db8:1:2::1"),
            parse_shared_network("2001:db8:1:3::1")
        );
        // "local"/garbage → None: skip, never guess.
        assert_eq!(parse_shared_network("local"), None);
        assert_eq!(parse_shared_network(""), None);
    }

    #[test]
    fn cidr_containment_boundaries() {
        let net = Cidr::parse("10.0.0.0/8").unwrap();
        assert!(net.contains("10.255.0.1".parse().unwrap()));
        assert!(!net.contains("203.0.113.7".parse().unwrap()));
        let host = Cidr::parse("192.168.1.5").unwrap(); // bare IP → /32
        assert!(host.contains("192.168.1.5".parse().unwrap()));
        assert!(!host.contains("192.168.1.6".parse().unwrap()));
        let all = Cidr::parse("0.0.0.0/0").unwrap();
        assert!(all.contains("203.0.113.7".parse().unwrap()));
        // Mixed families never match.
        assert!(!net.contains("2001:db8::1".parse().unwrap()));
        let v6 = Cidr::parse("2001:db8::/32").unwrap();
        assert!(v6.contains("2001:db8:ffff::1".parse().unwrap()));
        assert!(!v6.contains("2001:db9::1".parse().unwrap()));
    }

    #[test]
    fn allowlist_is_dormant_without_rules_and_closed_on_garbage() {
        assert!(matches!(
            evaluate_allowlist(&[], "whatever"),
            AllowlistVerdict::NoRules
        ));
        let rules = [Cidr::parse("10.0.0.0/8").unwrap()];
        assert!(matches!(
            evaluate_allowlist(&rules, "10.1.2.3"),
            AllowlistVerdict::Matches
        ));
        assert!(matches!(
            evaluate_allowlist(&rules, "203.0.113.7"),
            AllowlistVerdict::Denied
        ));
        // Unparseable source and proxy-less "local" fail CLOSED.
        assert!(matches!(
            evaluate_allowlist(&rules, "local"),
            AllowlistVerdict::Denied
        ));
        assert!(matches!(
            evaluate_allowlist(&rules, "not-an-ip"),
            AllowlistVerdict::Denied
        ));
    }
}
