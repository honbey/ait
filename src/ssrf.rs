use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use reqwest::Url;
use tracing::warn;

use crate::error::AitError;
use crate::middleware::CACHE_TTL;

enum SsrfDeny {
    NoHost,
    DnsFailed(String),
    Blocked,
}

/// Shared lookup + IP check.  Logs the block on [`SsrfDeny::Blocked`].
async fn resolve_and_check(
    url: &Url,
    allowed_cidrs: &[String],
    dns_cache: &Arc<DashMap<String, (Vec<IpAddr>, Instant)>>,
    provider_name: &str,
) -> Result<(), SsrfDeny> {
    let host = url.host_str().ok_or(SsrfDeny::NoHost)?;

    let ips = resolve_with_cache(host, dns_cache)
        .await
        .map_err(SsrfDeny::DnsFailed)?;

    for ip in &ips {
        if !is_allowed(ip, allowed_cidrs) {
            warn!(
                "[ssrf] blocked request to provider '{}' — {} resolves to private IP {}",
                provider_name, host, ip
            );
            return Err(SsrfDeny::Blocked);
        }
    }

    Ok(())
}

/// Pre-request SSRF check: opaque 502 on failure (proxy path).
pub(crate) async fn check_ssrf(
    url: &Url,
    allowed_cidrs: &[String],
    dns_cache: &Arc<DashMap<String, (Vec<IpAddr>, Instant)>>,
    provider_name: &str,
) -> Result<(), (axum::http::StatusCode, axum::Json<AitError>)> {
    resolve_and_check(url, allowed_cidrs, dns_cache, provider_name)
        .await
        .map_err(|_| AitError::upstream_error(502, "upstream request failed").into_response())
}

/// SSRF check for provider create/update: 400 with descriptive message.
pub(crate) async fn check_ssrf_config(
    url: &Url,
    allowed_cidrs: &[String],
    dns_cache: &Arc<DashMap<String, (Vec<IpAddr>, Instant)>>,
    provider_name: &str,
) -> Result<(), AitError> {
    match resolve_and_check(url, allowed_cidrs, dns_cache, provider_name).await {
        Ok(()) => Ok(()),
        Err(SsrfDeny::NoHost) => Err(AitError::bad_request("base_url must include a host")),
        Err(SsrfDeny::DnsFailed(e)) => {
            warn!("[ssrf] config check DNS error: {}", e);
            Err(AitError::bad_request("base_url host could not be resolved"))
        }
        Err(SsrfDeny::Blocked) => Err(AitError::bad_request(
            "base_url resolves to a blocked address",
        )),
    }
}

async fn resolve_with_cache(
    host: &str,
    cache: &Arc<DashMap<String, (Vec<IpAddr>, Instant)>>,
) -> Result<Vec<IpAddr>, String> {
    if let Some(entry) = cache.get(host)
        && entry.1.elapsed() < CACHE_TTL
    {
        return Ok(entry.0.clone());
    }

    let ips: Vec<IpAddr> = tokio::net::lookup_host((host, 0))
        .await
        .map_err(|e| format!("DNS lookup failed: {}", e))?
        .map(|addr| addr.ip())
        .collect();

    cache.insert(host.to_string(), (ips.clone(), Instant::now()));
    Ok(ips)
}

fn is_allowed(ip: &IpAddr, allowed_cidrs: &[String]) -> bool {
    let ip = normalize_ip(ip);
    for cidr in allowed_cidrs {
        if ip_in_cidr(&ip, cidr) {
            return true;
        }
    }
    !is_blocked_ip(&ip)
}

/// Canonicalize IPv6 forms that embed an IPv4 address (RFC 4291 IPv4-mapped
/// `::ffff:a.b.c.d` and RFC 6052 NAT64 `64:ff9b::/96`) into plain IPv4, so
/// blocked/CIDR checks see the address the connection actually reaches.
fn normalize_ip(ip: &IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => *ip,
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return IpAddr::V4(v4);
            }
            let s = v6.segments();
            if s[0] == 0x0064 && s[1] == 0xff9b && s[2] == 0 && s[3] == 0 {
                return IpAddr::V4(Ipv4Addr::new(
                    (s[6] >> 8) as u8,
                    (s[6] & 0xff) as u8,
                    (s[7] >> 8) as u8,
                    (s[7] & 0xff) as u8,
                ));
            }
            IpAddr::V6(*v6)
        }
    }
}

fn is_blocked_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_blocked_v4(ip),
        IpAddr::V6(ip) => is_blocked_v6(ip),
    }
}

fn is_blocked_v4(ip: &Ipv4Addr) -> bool {
    let o = ip.octets();
    match o[0] {
        0 => true,
        10 => true,
        127 => true,
        169 if o[1] == 254 => true,
        172 if (16..=31).contains(&o[1]) => true,
        192 if o[1] == 168 => true,
        100 if (64..=127).contains(&o[1]) => true,
        198 if o[1] == 18 => true,
        _ => false,
    }
}

fn is_blocked_v6(ip: &Ipv6Addr) -> bool {
    let s = ip.segments();
    // ::1 (loopback)
    if s[0] == 0
        && s[1] == 0
        && s[2] == 0
        && s[3] == 0
        && s[4] == 0
        && s[5] == 0
        && s[6] == 0
        && s[7] == 1
    {
        return true;
    }
    // fc00::/7 (unique local)
    if s[0] & 0xfe00 == 0xfc00 {
        return true;
    }
    // fe80::/10 (link-local)
    if s[0] & 0xffc0 == 0xfe80 {
        return true;
    }
    false
}

/// Check whether `ip` falls inside the CIDR range described by `cidr_str`.
///
/// A bare IP without a prefix length is treated as a /32 (IPv4) or /128 (IPv6).
pub(crate) fn ip_in_cidr(ip: &IpAddr, cidr_str: &str) -> bool {
    let (addr_str, prefix_len) = match cidr_str.split_once('/') {
        Some((a, p)) => (a, p.parse::<u8>().unwrap_or(32)),
        None => match ip {
            IpAddr::V4(_) => (cidr_str, 32),
            IpAddr::V6(_) => (cidr_str, 128),
        },
    };

    let cidr_addr = match IpAddr::from_str(addr_str) {
        Ok(a) => a,
        Err(_) => return false,
    };

    match (ip, cidr_addr) {
        (IpAddr::V4(ip), IpAddr::V4(cidr)) => ipv4_in_prefix(ip, &cidr, prefix_len.min(32)),
        (IpAddr::V6(ip), IpAddr::V6(cidr)) => ipv6_in_prefix(ip, &cidr, prefix_len.min(128)),
        _ => false,
    }
}

fn ipv4_in_prefix(ip: &Ipv4Addr, prefix: &Ipv4Addr, len: u8) -> bool {
    if len == 0 {
        return true;
    }
    let ip_bits = u32::from(*ip);
    let prefix_bits = u32::from(*prefix);
    let mask = !0u32 << (32 - len);
    (ip_bits & mask) == (prefix_bits & mask)
}

fn ipv6_in_prefix(ip: &Ipv6Addr, prefix: &Ipv6Addr, len: u8) -> bool {
    if len == 0 {
        return true;
    }
    let ip_bits = u128::from(*ip);
    let prefix_bits = u128::from(*prefix);
    let mask = !0u128 << (128 - len);
    (ip_bits & mask) == (prefix_bits & mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(ip: &str) -> bool {
        is_allowed(&ip.parse().unwrap(), &[])
    }

    #[test]
    fn v4_loopback_blocked() {
        assert!(!allowed("127.0.0.1"));
    }

    #[test]
    fn v6_loopback_blocked() {
        assert!(!allowed("::1"));
    }

    #[test]
    fn public_ipv4_allowed() {
        assert!(allowed("8.8.8.8"));
    }

    #[test]
    fn public_ipv6_allowed() {
        assert!(allowed("2001:db8::1"));
    }

    #[test]
    fn ipv4_mapped_loopback_blocked() {
        assert!(!allowed("::ffff:127.0.0.1"));
    }

    #[test]
    fn ipv4_mapped_metadata_blocked() {
        assert!(!allowed("::ffff:a9fe:a9fe"));
    }

    #[test]
    fn ipv4_mapped_public_allowed() {
        assert!(allowed("::ffff:8.8.8.8"));
    }

    #[test]
    fn nat64_metadata_blocked() {
        assert!(!allowed("64:ff9b::a9fe:a9fe"));
    }

    #[test]
    fn nat64_public_allowed() {
        assert!(allowed("64:ff9b::808:808"));
    }

    #[test]
    fn mapped_matches_v4_cidr_whitelist() {
        assert!(is_allowed(
            &"::ffff:192.168.3.130".parse().unwrap(),
            &["192.168.3.130".to_string()]
        ));
    }

    // ── is_blocked_v4 remaining arms ──

    #[test]
    fn v4_0_network_blocked() {
        assert!(!allowed("0.0.0.0"));
    }

    #[test]
    fn v4_10_private_blocked() {
        assert!(!allowed("10.0.0.1"));
    }

    #[test]
    fn v4_172_16_private_blocked() {
        assert!(!allowed("172.16.0.1"));
    }

    #[test]
    fn v4_172_31_private_blocked() {
        assert!(!allowed("172.31.255.255"));
    }

    #[test]
    fn v4_192_168_private_blocked() {
        assert!(!allowed("192.168.1.1"));
    }

    #[test]
    fn v4_100_64_carrier_nat_blocked() {
        assert!(!allowed("100.64.0.1"));
    }

    #[test]
    fn v4_198_18_testing_blocked() {
        assert!(!allowed("198.18.0.1"));
    }

    #[test]
    fn v4_169_254_linklocal_blocked() {
        assert!(!allowed("169.254.1.1"));
    }

    // ── is_blocked_v6 remaining arms ──

    #[test]
    fn v6_unique_local_blocked() {
        assert!(!allowed("fc00::1"));
    }

    #[test]
    fn v6_fd00_unique_local_blocked() {
        assert!(!allowed("fd12:3456:789a::1"));
    }

    #[test]
    fn v6_link_local_blocked() {
        assert!(!allowed("fe80::1"));
    }

    // ── ip_in_cidr / ipv4_in_prefix / ipv6_in_prefix ──

    #[test]
    fn ipv4_in_prefix_len_0_matches_all() {
        assert!(ip_in_cidr(&"8.8.8.8".parse().unwrap(), "0.0.0.0/0"));
    }

    #[test]
    fn ipv6_in_prefix_matches() {
        assert!(ip_in_cidr(&"2001:db8::1".parse().unwrap(), "2001:db8::/32"));
    }

    #[test]
    fn ipv6_in_prefix_mismatch() {
        assert!(!ip_in_cidr(
            &"2001:dead::1".parse().unwrap(),
            "2001:db8::/32"
        ));
    }

    #[test]
    fn ipv6_bare_ip_treated_as_128() {
        assert!(ip_in_cidr(&"2001:db8::1".parse().unwrap(), "2001:db8::1"));
        assert!(!ip_in_cidr(&"2001:db8::2".parse().unwrap(), "2001:db8::1"));
    }

    #[test]
    fn ip_in_cidr_invalid_cidr_returns_false() {
        assert!(!ip_in_cidr(
            &"8.8.28"
                .parse::<IpAddr>()
                .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
            "not-an-ip"
        ));
    }

    #[test]
    fn ip_in_cidr_v4_v6_mismatch_returns_false() {
        assert!(!ip_in_cidr(&"8.8.8.8".parse().unwrap(), "2001:db8::/32"));
    }

    #[test]
    fn ipv6_in_prefix_len_0_matches_all() {
        assert!(ip_in_cidr(&"2001:db8::1".parse().unwrap(), "::/0"));
    }

    // ── check_ssrf / check_ssrf_config ──

    #[tokio::test]
    async fn check_ssrf_config_no_host_returns_bad_request() {
        let dns_cache = Arc::new(DashMap::new());
        let url = reqwest::Url::parse("http:///no-host").unwrap();
        let result =
            check_ssrf_config(&url, &["127.0.0.1/8".to_string()], &dns_cache, "test").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("host"));
    }

    #[tokio::test]
    async fn check_ssrf_config_blocked_returns_bad_request() {
        let dns_cache: Arc<DashMap<String, (Vec<IpAddr>, Instant)>> = Arc::new(DashMap::new());
        dns_cache.insert(
            "internal.test".to_string(),
            (vec!["10.0.0.1".parse().unwrap()], Instant::now()),
        );
        let url = reqwest::Url::parse("http://internal.test/api").unwrap();
        let result = check_ssrf_config(&url, &[], &dns_cache, "test").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("blocked"));
    }

    #[tokio::test]
    async fn check_ssrf_blocked_returns_502() {
        let dns_cache: Arc<DashMap<String, (Vec<IpAddr>, Instant)>> = Arc::new(DashMap::new());
        dns_cache.insert(
            "internal.test".to_string(),
            (vec!["10.0.0.1".parse().unwrap()], Instant::now()),
        );
        let url = reqwest::Url::parse("http://internal.test/api").unwrap();
        let result = check_ssrf(&url, &[], &dns_cache, "test").await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, axum::http::StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn check_ssrf_config_allowed_returns_ok() {
        let dns_cache: Arc<DashMap<String, (Vec<IpAddr>, Instant)>> = Arc::new(DashMap::new());
        dns_cache.insert(
            "allowed.test".to_string(),
            (vec!["127.0.0.1".parse().unwrap()], Instant::now()),
        );
        let url = reqwest::Url::parse("http://allowed.test/api").unwrap();
        let result =
            check_ssrf_config(&url, &["127.0.0.1/8".to_string()], &dns_cache, "test").await;
        assert!(result.is_ok());
    }
}
