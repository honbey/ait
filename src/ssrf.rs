use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use reqwest::Url;
use tracing::warn;

use crate::error::AitError;
use crate::middleware::CACHE_TTL;

/// Pre-request SSRF check: resolve hostname to IPs and verify each is
/// either a public address or explicitly allowed via `allowed_cidrs`.
pub(crate) async fn check_ssrf(
    url: &Url,
    allowed_cidrs: &[String],
    dns_cache: &Arc<DashMap<String, (Vec<IpAddr>, Instant)>>,
    provider_name: &str,
) -> Result<(), (axum::http::StatusCode, axum::Json<AitError>)> {
    let host = url
        .host_str()
        .ok_or_else(|| AitError::upstream_error(502, "upstream request failed").into_response())?;

    let ips = resolve_with_cache(host, dns_cache).await.map_err(|_| {
        AitError::upstream_error(
            502,
            format!(
                "Failed to connect to provider '{}': connection refused",
                provider_name
            ),
        )
        .into_response()
    })?;

    for ip in &ips {
        if !is_allowed(ip, allowed_cidrs) {
            warn!(
                "[ssrf] blocked request to provider '{}' — {} resolves to private IP {}",
                provider_name, host, ip
            );
            return Err(AitError::upstream_error(502, "upstream request failed").into_response());
        }
    }

    Ok(())
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
    for cidr in allowed_cidrs {
        if ip_in_cidr(ip, cidr) {
            return true;
        }
    }
    !is_blocked_ip(ip)
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
