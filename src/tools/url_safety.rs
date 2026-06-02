//! SSRF guards for the `web` tool — block loopback, private, and metadata targets.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use reqwest::Url;

/// Returns `Ok(())` when `url` is safe to fetch (public HTTP(S) only).
pub async fn validate_public_http_url(url: &str) -> Result<(), String> {
    let parsed = Url::parse(url).map_err(|e| format!("invalid url: {e}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("url must be http:// or https://".to_owned());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "url must include a host".to_owned())?;

    if is_blocked_hostname(host) {
        return Err(format!("blocked host: {host}"));
    }

    if let Some(port) = parsed.port() {
        if port == 0 {
            return Err("blocked port".to_owned());
        }
    }

    // Literal IP in the host (including decimal/octal/hex encodings via parser).
    if let Some(ip) = parsed.host().and_then(|h| match h {
        url::Host::Ipv4(v) => Some(IpAddr::V4(v)),
        url::Host::Ipv6(v) => Some(IpAddr::V6(v)),
        url::Host::Domain(_) => None,
    }) {
        if is_blocked_ip(ip) {
            return Err(format!("blocked address: {ip}"));
        }
        return Ok(());
    }

    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs: Vec<_> = match tokio::net::lookup_host((host, port)).await {
        Ok(iter) => iter.map(|a| a.ip()).collect(),
        Err(e) => return Err(format!("dns lookup failed for {host}: {e}")),
    };
    if addrs.is_empty() {
        return Err(format!("dns lookup returned no addresses for {host}"));
    }
    for ip in addrs {
        if is_blocked_ip(ip) {
            return Err(format!("blocked resolved address: {ip}"));
        }
    }
    Ok(())
}

fn is_blocked_hostname(host: &str) -> bool {
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if h.is_empty() {
        return true;
    }
    if h == "localhost" || h.ends_with(".localhost") {
        return true;
    }
    if h.ends_with(".local") || h.ends_with(".internal") {
        return true;
    }
    if h == "metadata" || h == "metadata.google.internal" {
        return true;
    }
    if let Ok(ip) = parse_literal_ip(&h) {
        return is_blocked_ip(ip);
    }
    false
}

fn parse_literal_ip(host: &str) -> Result<IpAddr, ()> {
    if host.starts_with('[') && host.ends_with(']') {
        return Ipv6Addr::from_str(&host[1..host.len() - 1])
            .map(IpAddr::V6)
            .map_err(|_| ());
    }
    host.parse::<IpAddr>().map_err(|_| ())
}

pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_ipv4(v4),
        IpAddr::V6(v6) => is_blocked_ipv6(v6),
    }
}

fn is_blocked_ipv4(v4: Ipv4Addr) -> bool {
    if v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
    {
        return true;
    }
    // AWS/GCP/Azure link-local metadata
    if v4.octets() == [169, 254, 169, 254] {
        return true;
    }
    // 0.0.0.0/8 — "this network"
    if v4.octets()[0] == 0 {
        return true;
    }
    // Carrier-grade NAT / shared address space 100.64.0.0/10
    let o = v4.octets();
    if o[0] == 100 && (o[1] & 0xC0) == 64 {
        return true;
    }
    false
}

fn is_blocked_ipv6(v6: Ipv6Addr) -> bool {
    if v6.is_loopback() || v6.is_unspecified() {
        return true;
    }
    if v6.is_multicast() {
        return true;
    }
    // Unique local fc00::/7
    let segs = v6.segments();
    if (segs[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    // Link-local fe80::/10
    if (segs[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_loopback_and_private_ipv4() {
        assert!(is_blocked_ip("127.0.0.1".parse().unwrap()));
        assert!(is_blocked_ip("10.0.0.1".parse().unwrap()));
        assert!(is_blocked_ip("192.168.1.1".parse().unwrap()));
        assert!(is_blocked_ip("169.254.169.254".parse().unwrap()));
    }

    #[test]
    fn allows_public_ipv4() {
        assert!(!is_blocked_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_blocked_ip("1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn blocks_localhost_hostname() {
        assert!(is_blocked_hostname("localhost"));
        assert!(is_blocked_hostname("app.localhost"));
        assert!(is_blocked_hostname("metadata.google.internal"));
    }

    #[tokio::test]
    async fn rejects_loopback_url_before_fetch() {
        let err = validate_public_http_url("http://127.0.0.1/")
            .await
            .unwrap_err();
        assert!(err.contains("blocked"));
    }

    #[tokio::test]
    async fn allows_public_host() {
        validate_public_http_url("https://example.com/")
            .await
            .expect("example.com should resolve to public IPs");
    }
}
