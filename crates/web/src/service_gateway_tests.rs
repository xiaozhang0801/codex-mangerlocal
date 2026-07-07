use super::{
    format_upstream_error_message, gateway_proxy_max_body_bytes, gateway_proxy_target_url,
    service_probe_client, should_skip_gateway_request_header, should_skip_gateway_response_header,
    ENV_GATEWAY_PROXY_MAX_BODY_BYTES,
};
use axum::http::{header, HeaderName, HeaderValue, Uri};
use std::sync::{Mutex, MutexGuard};

static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

fn env_test_lock() -> MutexGuard<'static, ()> {
    ENV_TEST_LOCK.lock().expect("env test lock")
}

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }

    fn clear(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[test]
fn format_upstream_error_message_adds_docker_hint_for_host_internal() {
    let err = std::io::Error::other("dns failed");
    let message = format_upstream_error_message("host.docker.internal:9760", &err);
    assert!(message.contains("host.docker.internal"));
    assert!(message.contains("codexmanager-service:48760"));
}

#[test]
fn service_probe_client_builds_with_dedicated_config_and_reuses_cache() {
    let first = service_probe_client().expect("build first probe client");
    let second = service_probe_client().expect("reuse probe client");
    drop((first, second));
}

#[test]
fn gateway_proxy_target_url_preserves_path_and_query() {
    let uri: Uri = "/v1/responses?stream=true".parse().expect("valid uri");
    assert_eq!(
        gateway_proxy_target_url("localhost:48760", &uri),
        "http://localhost:48760/v1/responses?stream=true"
    );
}

#[test]
fn gateway_proxy_body_limit_defaults_to_unbounded() {
    let _lock = env_test_lock();
    let _guard = EnvGuard::clear(ENV_GATEWAY_PROXY_MAX_BODY_BYTES);

    assert_eq!(gateway_proxy_max_body_bytes(), 0);
}

#[test]
fn gateway_proxy_body_limit_allows_env_override() {
    let _lock = env_test_lock();
    let _guard = EnvGuard::set(ENV_GATEWAY_PROXY_MAX_BODY_BYTES, "536870912");

    assert_eq!(gateway_proxy_max_body_bytes(), 536_870_912);
}

#[test]
fn gateway_proxy_header_filters_skip_hop_by_hop_headers() {
    assert!(should_skip_gateway_request_header(
        &header::HOST,
        &HeaderValue::from_static("example.com")
    ));
    assert!(should_skip_gateway_response_header(&header::CONTENT_LENGTH));
    assert!(!should_skip_gateway_request_header(
        &header::AUTHORIZATION,
        &HeaderValue::from_static("Bearer key")
    ));
}

#[test]
fn gateway_proxy_header_filters_drop_forwarded_client_ip_spoofing() {
    assert!(should_skip_gateway_request_header(
        &HeaderName::from_static(codexmanager_service::FORWARDED_CLIENT_IP_HEADER),
        &HeaderValue::from_static("10.0.0.99")
    ));
}
