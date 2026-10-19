use super::*;

#[test]
fn password_auth_rejects_a_hash_removed_after_mode_was_read() {
    let mut headers = HeaderMap::new();
    assert!(!password_request_is_authenticated(
        &headers,
        None,
        "rpc-token",
        "session-key",
    ));

    let old_cookie = build_web_auth_cookie_value("old-hash", "rpc-token", "session-key");
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!("{WEB_AUTH_COOKIE_NAME}={old_cookie}")).unwrap(),
    );
    assert!(!password_request_is_authenticated(
        &headers,
        None,
        "rpc-token",
        "session-key",
    ));
}

#[test]
fn password_auth_accepts_only_a_cookie_for_the_current_hash_and_session() {
    let mut headers = HeaderMap::new();
    assert!(!password_request_is_authenticated(
        &headers,
        Some("current-hash"),
        "rpc-token",
        "session-key",
    ));
    let cookie = build_web_auth_cookie_value("current-hash", "rpc-token", "session-key");
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!("{WEB_AUTH_COOKIE_NAME}={cookie}")).unwrap(),
    );
    assert!(password_request_is_authenticated(
        &headers,
        Some("current-hash"),
        "rpc-token",
        "session-key",
    ));
    assert!(!password_request_is_authenticated(
        &headers,
        Some("new-hash"),
        "rpc-token",
        "session-key",
    ));
    assert!(!password_request_is_authenticated(
        &headers,
        Some("current-hash"),
        "rpc-token",
        "new-session-key",
    ));
}

/// 函数 `login_force_requested_accepts_truthy_flags`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn login_force_requested_accepts_truthy_flags() {
    for value in ["1", "true", "TRUE", "yes", "on"] {
        let query = LoginQuery {
            force: Some(value.to_string()),
        };
        assert!(login_force_requested(&query), "value={value}");
    }
    for value in ["", "0", "false", "no", "off"] {
        let query = LoginQuery {
            force: Some(value.to_string()),
        };
        assert!(!login_force_requested(&query), "value={value}");
    }
    assert!(!login_force_requested(&LoginQuery::default()));
}

/// 函数 `login_success_html_marks_current_tab_session`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn login_success_html_marks_current_tab_session() {
    let html = login_success_html();
    assert!(html.contains("sessionStorage.setItem"));
    assert!(html.contains(WEB_AUTH_TAB_SESSION_STORAGE_KEY));
    assert!(html.contains("location.replace(\"/\")"));
}

#[test]
fn web_auth_allows_static_assets_without_session() {
    for path in [
        "/_next/static/chunks/app/page.js",
        "/_next/static/css/app.css",
        "/favicon.ico",
        "/author-alipay.jpg",
        "/manifest.json",
    ] {
        assert!(is_public_static_asset_path(path), "path={path}");
    }
    assert!(!is_public_static_asset_path("/settings"));
    assert!(!is_public_static_asset_path("/api/rpc"));
}
