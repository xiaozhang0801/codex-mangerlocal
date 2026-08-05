use std::net::SocketAddr;

use axum::http::HeaderMap;
use codexmanager_service::client_ip::{
    resolve_trusted_client_ip, set_forwarded_client_ip_header, FORWARDED_CLIENT_IP_HEADER,
};

#[test]
fn loopback_remote_trusts_forwarded_client_ip() {
    let remote: SocketAddr = "127.0.0.1:48760".parse().unwrap();

    assert_eq!(
        resolve_trusted_client_ip(Some(&remote), Some("192.168.1.20")),
        Some("192.168.1.20".to_string()),
    );
}

#[test]
fn non_loopback_remote_ignores_forwarded_client_ip() {
    let remote: SocketAddr = "10.0.0.10:48760".parse().unwrap();

    assert_eq!(
        resolve_trusted_client_ip(Some(&remote), Some("192.168.1.20")),
        Some("10.0.0.10".to_string()),
    );
}

#[test]
fn forwarded_client_ip_header_replaces_external_value() {
    let mut headers = HeaderMap::new();
    headers.insert(FORWARDED_CLIENT_IP_HEADER, "203.0.113.9".parse().unwrap());
    let peer_addr: SocketAddr = "192.168.1.20:55331".parse().unwrap();

    set_forwarded_client_ip_header(&mut headers, peer_addr);

    assert_eq!(
        headers
            .get(FORWARDED_CLIENT_IP_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some("192.168.1.20"),
    );
}
