use std::net::{IpAddr, SocketAddr};

use axum::http::{HeaderMap, HeaderValue};

pub const FORWARDED_CLIENT_IP_HEADER: &str = "x-codexmanager-client-ip";

fn parse_forwarded_client_ip(value: &str) -> Option<IpAddr> {
    value
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<IpAddr>().ok())
}

pub fn resolve_trusted_client_ip(
    remote_addr: Option<&SocketAddr>,
    forwarded_client_ip: Option<&str>,
) -> Option<String> {
    let remote_ip = remote_addr.map(SocketAddr::ip)?;
    if remote_ip.is_loopback() {
        if let Some(forwarded_ip) = forwarded_client_ip.and_then(parse_forwarded_client_ip) {
            return Some(forwarded_ip.to_string());
        }
    }
    Some(remote_ip.to_string())
}

pub fn set_forwarded_client_ip_header(headers: &mut HeaderMap, peer_addr: SocketAddr) {
    headers.remove(FORWARDED_CLIENT_IP_HEADER);
    if let Ok(value) = HeaderValue::from_str(&peer_addr.ip().to_string()) {
        headers.insert(FORWARDED_CLIENT_IP_HEADER, value);
    }
}
