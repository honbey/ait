use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use reqwest::Url;
use tracing::warn;

use crate::app::AppState;
use crate::error::AitError;
use crate::middleware::CACHE_TTL;

enum SsrfDeny {
    NoHost,
    DnsFailed(String),
    Blocked,
}

/// Shared lookup + IP check. Returns the verified IP set so callers can pin
/// the outgoing connection to it (see [`pinned_client`]).
/// Logs the block on [`SsrfDeny::Blocked`].
async fn resolve_and_check(
    url: &Url,
    allowed_cidrs: &[String],
    dns_cache: &Arc<DashMap<String, (Vec<IpAddr>, Instant)>>,
    provider_name: &str,
) -> Result<Vec<IpAddr>, SsrfDeny> {
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

    Ok(ips)
}

/// Pre-request SSRF check: opaque 502 on failure (proxy path). Returns the
/// verified IP set for connection pinning via [`pinned_client`].
pub(crate) async fn check_ssrf(
    url: &Url,
    allowed_cidrs: &[String],
    dns_cache: &Arc<DashMap<String, (Vec<IpAddr>, Instant)>>,
    provider_name: &str,
) -> Result<Vec<IpAddr>, (axum::http::StatusCode, axum::Json<AitError>)> {
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
        Ok(_) => Ok(()),
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

/// Return a client whose DNS for `url.host` is pinned to `verified_ips`.
///
/// [`check_ssrf`] validates the IPs the host resolves to, but reqwest would
/// resolve DNS again when actually connecting — a window an attacker with DNS
/// control can abuse (DNS rebinding). Pinning the client to the verified set
/// closes that window. Clients are cached per `host:port` and rebuilt once the
/// cache entry ages past `CACHE_TTL`, mirroring the DNS cache lifetime.
pub(crate) fn pinned_client(
    state: &AppState,
    url: &Url,
    verified_ips: &[IpAddr],
) -> Result<reqwest::Client, (axum::http::StatusCode, axum::Json<AitError>)> {
    let opaque_error = || AitError::upstream_error(502, "upstream request failed").into_response();

    let host = url.host_str().ok_or_else(opaque_error)?;
    let port = url.port_or_known_default().unwrap_or(80);
    // Url::host_str renders IPv6 hosts with brackets; reqwest's resolve
    // matches the bare address form.
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let cache_key = format!("{host}:{port}");

    // Reuse while fresh; drop the guard before any later insert on the same
    // map (parking_lot RwLock is not reentrant).
    if let Some(entry) = state.pinned_clients.get(&cache_key) {
        if entry.1.elapsed() < CACHE_TTL {
            let client = entry.0.clone();
            drop(entry);
            return Ok(client);
        }
        drop(entry);
    }

    if verified_ips.is_empty() {
        // A pinned client without verified addresses would silently fall back
        // to real DNS at connect time, defeating the SSRF check.
        return Err(opaque_error());
    }

    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(state.config.proxy.connect_timeout_secs))
        .tcp_keepalive(Some(Duration::from_secs(60)));
    for ip in verified_ips {
        builder = builder.resolve(host, SocketAddr::new(*ip, port));
    }
    let client = builder.build().map_err(|e| {
        warn!("[ssrf] pinned client build failed: {e}");
        opaque_error()
    })?;

    state
        .pinned_clients
        .insert(cache_key, (client.clone(), Instant::now()));
    Ok(client)
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
        Some((a, p)) => (a, Some(p)),
        None => (cidr_str, None),
    };

    let cidr_addr = match IpAddr::from_str(addr_str) {
        Ok(a) => a,
        Err(_) => return false,
    };

    // The default (and cap) follow the CIDR's own address family. Falling back
    // to a bare 32 would turn an IPv6 entry with an unparseable prefix into a
    // /32, matching far more than intended; such an entry fails closed.
    let family_max = match cidr_addr {
        IpAddr::V4(_) => 32u8,
        IpAddr::V6(_) => 128u8,
    };
    let prefix_len = match prefix_len {
        Some(p) => match p.parse::<u8>() {
            Ok(len) if len <= family_max => len,
            _ => return false,
        },
        None => family_max,
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
    fn ip_in_cidr_prefix_follows_cidr_family() {
        // An unparseable or out-of-range prefix fails closed instead of
        // degrading to an IPv4 /32, which would widen an IPv6 entry a lot.
        assert!(!ip_in_cidr(
            &"2001:db8::1".parse().unwrap(),
            "2001:db8::/abc"
        ));
        assert!(!ip_in_cidr(&"10.0.0.1".parse().unwrap(), "10.0.0.0/abc"));
        assert!(!ip_in_cidr(&"10.0.0.1".parse().unwrap(), "10.0.0.0/33"));
        // A bare IPv6 address defaults to /128, not /32.
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

    // ── pinned_client ──

    #[test]
    fn pinned_client_caches_per_host_and_port() {
        let (state, _dir) = crate::test_utils::create_test_state();
        let ips = vec!["127.0.0.1".parse().unwrap()];

        let url = reqwest::Url::parse("http://upstream.test/api").unwrap();
        let _first = pinned_client(&state, &url, &ips).unwrap();
        let _second = pinned_client(&state, &url, &ips).unwrap();
        assert_eq!(state.pinned_clients.len(), 1, "same host:port is reused");

        let url_8080 = reqwest::Url::parse("http://upstream.test:8080/api").unwrap();
        let _third = pinned_client(&state, &url_8080, &ips).unwrap();
        assert_eq!(
            state.pinned_clients.len(),
            2,
            "different port gets its own entry"
        );
    }

    #[test]
    fn pinned_client_strips_ipv6_brackets() {
        let (state, _dir) = crate::test_utils::create_test_state();
        let url = reqwest::Url::parse("http://[::1]/api").unwrap();
        let ips = vec!["::1".parse().unwrap()];
        let _ = pinned_client(&state, &url, &ips).unwrap();
        // The cache key must use the bare address form; a bracketed key would
        // miss on subsequent lookups and rebuild a client per request.
        assert_eq!(state.pinned_clients.len(), 1);
    }

    #[test]
    fn pinned_client_without_verified_ips_is_rejected() {
        let (state, _dir) = crate::test_utils::create_test_state();
        let url = reqwest::Url::parse("http://upstream.test/api").unwrap();
        let result = pinned_client(&state, &url, &[]);
        assert!(result.is_err(), "must not fall back to real DNS");
        assert!(state.pinned_clients.is_empty());
    }
}
