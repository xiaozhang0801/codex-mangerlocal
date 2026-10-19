//! End-to-end setting-write contracts. Both service storage dispatch paths run
//! against disposable local SQLite files; no external database is contacted.

use codexmanager_service as service;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod support;
use support::{test_env_guard, EnvGuard};

const TEST_CHILD: &str = "CODEXMANAGER_Q03_SETTINGS_TEST_CHILD";
const TEST_DIR: &str = "CODEXMANAGER_Q03_SETTINGS_TEST_DIR";

const RUNTIME_ENV_KEYS: &[&str] = &[
    "CODEXMANAGER_SERVICE_ADDR",
    "CODEXMANAGER_WEB_ADDR",
    "CODEXMANAGER_ROUTE_STRATEGY",
    "CODEXMANAGER_ORIGINATOR",
    "CODEXMANAGER_FREE_ACCOUNT_MAX_MODEL",
    "CODEXMANAGER_MODEL_FORWARD_RULES",
    "CODEXMANAGER_COMPACT_MODEL_FORWARD_RULES",
    "CODEXMANAGER_UPSTREAM_PROXY_URL",
    "CODEXMANAGER_UPSTREAM_PROXY_BYPASS_HOSTS",
    "CODEXMANAGER_PROXY_LIST",
    "CODEXMANAGER_ACCOUNT_MAX_INFLIGHT",
    "CODEXMANAGER_UPSTREAM_CONNECT_TIMEOUT_SECS",
    "CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS",
    "CODEXMANAGER_Q03_ENV_ROUND",
    "CODEXMANAGER_QUOTA_GUARD_ENABLED",
    "CODEXMANAGER_QUOTA_GUARD_5H_MIN_REMAINING_PERCENT",
    "CODEXMANAGER_QUOTA_GUARD_WEEKLY_MIN_REMAINING_PERCENT",
    "CODEXMANAGER_QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK",
    "CODEXMANAGER_SSE_KEEPALIVE_ENABLED",
    "CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS",
    "CODEXMANAGER_USAGE_POLLING_ENABLED",
    "CODEXMANAGER_USAGE_POLL_INTERVAL_SECS",
    "CODEXMANAGER_WARMUP_CRON_ENABLED",
    "CODEXMANAGER_WARMUP_CRON_EXPRESSION",
    "CODEXMANAGER_GATEWAY_KEEPALIVE_ENABLED",
    "CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS",
    "CODEXMANAGER_TOKEN_REFRESH_POLLING_ENABLED",
    "CODEXMANAGER_TOKEN_REFRESH_POLL_INTERVAL_SECS",
    "CODEXMANAGER_TOKEN_REFRESH_AHEAD_SECS",
    "CODEXMANAGER_USAGE_REFRESH_WORKERS",
    "CODEXMANAGER_HTTP_WORKER_FACTOR",
    "CODEXMANAGER_HTTP_WORKER_MIN",
    "CODEXMANAGER_HTTP_STREAM_WORKER_FACTOR",
    "CODEXMANAGER_HTTP_STREAM_WORKER_MIN",
];

struct ScopedEnv(Vec<(&'static str, Option<OsString>)>);

impl ScopedEnv {
    fn clear(keys: &[&'static str]) -> Self {
        let previous = keys
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect();
        for key in keys {
            std::env::remove_var(key);
        }
        Self(previous)
    }
}

impl Drop for ScopedEnv {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "codexmanager-q03-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("create owned temporary directory");
        Self(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        // This path is created exclusively by this test, never supplied by a
        // user. The child has exited before its parent removes the file.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn save(patch: Value) -> Value {
    service::app_settings_set(Some(&patch)).expect("save test settings")
}

fn runtime_snapshot() -> Value {
    // An empty patch returns the public post-write snapshot without a storage
    // reload. A getter reload could conceal a runtime value published too soon.
    save(json!({}))
}

fn baseline() -> Value {
    save(json!({
        "updateAutoCheck": true,
        "routeStrategy": "balanced",
        "gatewayOriginator": "q03-baseline",
        "gatewayUserAgent": "",
        "gatewayUserAgentVersion": service::default_gateway_user_agent_version(),
        "upstreamProxyUrl": "",
        "quotaGuard": {
            "enabled": true,
            "primaryMinRemainingPercent": 5,
            "secondaryMinRemainingPercent": 10,
            "allowAllLowQuotaFallback": true
        },
        "backgroundTasks": {
            "usagePollingEnabled": true,
            "gatewayKeepaliveEnabled": true,
            "tokenRefreshPollingEnabled": true,
            "warmupCronEnabled": false,
            "httpWorkerMin": 8,
            "usageRefreshWorkers": 4
        }
    }))
}

type SettingRows = BTreeMap<String, (String, i64)>;

fn rows(connection: &Connection) -> SettingRows {
    let mut query = connection
        .prepare("SELECT key, value, updated_at FROM app_settings ORDER BY key")
        .expect("prepare settings read");
    query
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?)))
        })
        .expect("read settings")
        .collect::<Result<SettingRows, _>>()
        .expect("decode settings")
}

fn process_runtime_env() -> Vec<Option<OsString>> {
    RUNTIME_ENV_KEYS.iter().map(std::env::var_os).collect()
}

fn reject_writes_to(connection: &Connection, key: &str) {
    for event in ["INSERT", "UPDATE"] {
        connection
            .execute(
                &format!(
                    "CREATE TRIGGER q03_reject_{event} BEFORE {event} ON app_settings \
                     WHEN NEW.key = '{key}' \
                     BEGIN SELECT RAISE(ABORT, 'q03 injected settings failure'); END"
                ),
                [],
            )
            .expect("install settings-write failure trigger");
    }
}

fn allow_writes(connection: &Connection) {
    for event in ["INSERT", "UPDATE"] {
        connection
            .execute(&format!("DROP TRIGGER q03_reject_{event}"), [])
            .expect("remove settings-write failure trigger");
    }
}

fn concurrent_patches(left: Value, right: Value) -> [Result<Value, String>; 2] {
    let barrier = Arc::new(Barrier::new(3));
    let handles = [left, right].map(|patch| {
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            service::app_settings_set(Some(&patch))
        })
    });
    barrier.wait();
    handles.map(|handle| handle.join().expect("settings worker did not panic"))
}

fn assert_rejected_without_changes(connection: &Connection, patch: Value) -> String {
    let persisted_before = rows(connection);
    let runtime_before = runtime_snapshot();
    let environment_before = process_runtime_env();
    let err = service::app_settings_set(Some(&patch)).expect_err("reject invalid patch");
    assert_eq!(
        rows(connection),
        persisted_before,
        "rejected patch wrote settings"
    );
    assert_eq!(
        runtime_snapshot(),
        runtime_before,
        "rejected patch changed runtime"
    );
    assert_eq!(
        process_runtime_env(),
        environment_before,
        "rejected patch changed env"
    );
    err
}

fn mixed_invalid_fields_are_atomic(connection: &Connection) {
    baseline();
    for invalid in [
        json!({"updateAutoCheck": false, "routeStrategy": "invalid"}),
        json!({"gatewayOriginator": "should-not-publish", "gatewayUserAgent": "bad\r\nvalue"}),
        json!({"updateAutoCheck": false, "sseKeepaliveIntervalMs": 0}),
        json!({"updateAutoCheck": false, "modelForwardRules": "invalid"}),
        json!({"updateAutoCheck": false, "upstreamProxyBypassHosts": "bad\u{0000}host"}),
        json!({"updateAutoCheck": false, "upstreamProxyUrl": "http://127.0.0.1:8888/\u{0000}"}),
        json!({"updateAutoCheck": false, "serviceAddr": "127.0.0.1:48760\u{0000}"}),
    ] {
        assert_rejected_without_changes(connection, invalid);
    }
}

fn failed_persistence_rolls_back_every_field(connection: &Connection) {
    baseline();
    reject_writes_to(
        connection,
        service::APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY,
    );
    assert_rejected_without_changes(
        connection,
        json!({
            "updateAutoCheck": false,
            "routeStrategy": "ordered",
            "gatewayOriginator": "q03-uncommitted-originator",
            "upstreamProxyUrl": "http://127.0.0.1:31999",
            "quotaGuard": {"primaryMinRemainingPercent": 31},
            "backgroundTasks": {"usagePollingEnabled": false}
        }),
    );
    allow_writes(connection);
}

fn concurrent_different_keys_do_not_rewrite_snapshots(connection: &Connection) {
    baseline();
    connection
        .execute(
            "INSERT INTO app_settings(key, value, updated_at) VALUES('ui.theme', 'raw-theme-sentinel', 1) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
            [],
        )
        .expect("seed an unrelated value that the response normalizes");
    let before = rows(connection);
    connection
        .execute("CREATE TABLE q03_setting_writes(key TEXT NOT NULL)", [])
        .expect("create write audit");
    for event in ["INSERT", "UPDATE", "DELETE"] {
        let key = if event == "DELETE" {
            "OLD.key"
        } else {
            "NEW.key"
        };
        connection
            .execute(
                &format!(
                    "CREATE TRIGGER q03_audit_{event} AFTER {event} ON app_settings \
                     BEGIN INSERT INTO q03_setting_writes(key) VALUES({key}); END"
                ),
                [],
            )
            .expect("audit every persisted key");
    }
    let responses = concurrent_patches(
        json!({"updateAutoCheck": false}),
        json!({"gatewayOriginator": "q03-concurrent-originator"}),
    );
    for response in responses {
        assert!(response.is_ok(), "concurrent patch failed: {response:?}");
    }
    let after = rows(connection);
    assert_eq!(after[service::APP_SETTING_UPDATE_AUTO_CHECK_KEY].0, "0");
    assert_eq!(
        after[service::APP_SETTING_GATEWAY_ORIGINATOR_KEY].0,
        "q03-concurrent-originator"
    );
    for (key, value) in before {
        if key != service::APP_SETTING_UPDATE_AUTO_CHECK_KEY
            && key != service::APP_SETTING_GATEWAY_ORIGINATOR_KEY
        {
            assert_eq!(
                after.get(&key),
                Some(&value),
                "unrelated key rewritten: {key}"
            );
        }
    }
    let mut query = connection
        .prepare("SELECT key FROM q03_setting_writes ORDER BY key")
        .expect("read write audit");
    let actual = query
        .query_map([], |row| row.get::<_, String>(0))
        .expect("read audited keys")
        .collect::<Result<Vec<_>, _>>()
        .expect("decode audited keys");
    assert_eq!(
        actual,
        vec![
            service::APP_SETTING_UPDATE_AUTO_CHECK_KEY.to_string(),
            service::APP_SETTING_GATEWAY_ORIGINATOR_KEY.to_string(),
        ],
        "post-write response persisted settings outside its patch"
    );
    drop(query);
    for event in ["INSERT", "UPDATE", "DELETE"] {
        connection
            .execute(&format!("DROP TRIGGER q03_audit_{event}"), [])
            .unwrap();
    }
    connection
        .execute("DROP TABLE q03_setting_writes", [])
        .unwrap();
}

fn concurrent_nested_patches_merge(connection: &Connection) {
    for turn in 0..6 {
        baseline();
        let responses = concurrent_patches(
            json!({
                "quotaGuard": {"primaryMinRemainingPercent": 17 + turn},
                "backgroundTasks": {"usagePollingEnabled": false}
            }),
            json!({
                "quotaGuard": {"secondaryMinRemainingPercent": 29 + turn},
                "backgroundTasks": {"gatewayKeepaliveEnabled": false}
            }),
        );
        for response in responses {
            assert!(response.is_ok(), "nested patch failed: {response:?}");
        }
        let snapshot = runtime_snapshot();
        assert_eq!(
            snapshot["quotaGuard"]["primaryMinRemainingPercent"],
            json!(17.0 + f64::from(turn))
        );
        assert_eq!(
            snapshot["quotaGuard"]["secondaryMinRemainingPercent"],
            json!(29.0 + f64::from(turn))
        );
        assert_eq!(snapshot["backgroundTasks"]["usagePollingEnabled"], false);
        assert_eq!(
            snapshot["backgroundTasks"]["gatewayKeepaliveEnabled"],
            false
        );
        let persisted = rows(connection);
        let quota: Value =
            serde_json::from_str(&persisted[service::APP_SETTING_GATEWAY_QUOTA_GUARD_KEY].0)
                .unwrap();
        let background: Value =
            serde_json::from_str(&persisted[service::APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY].0)
                .unwrap();
        assert_eq!(quota, snapshot["quotaGuard"]);
        assert_eq!(background, snapshot["backgroundTasks"]);
    }
}

fn concurrent_reads_do_not_overwrite_committed_patches(connection: &Connection) {
    baseline();
    save(json!({"sseKeepaliveIntervalMs": 15000}));
    let barrier = Arc::new(Barrier::new(4));
    let (finished, completed) = mpsc::channel();
    let reader_barrier = Arc::clone(&barrier);
    let reader_finished = finished.clone();
    let reader = std::thread::spawn(move || {
        reader_barrier.wait();
        let result = (|| -> Result<(), String> {
            for iteration in 0..24 {
                let snapshot = if iteration % 2 == 0 {
                    service::app_settings_get()?
                } else {
                    service::app_settings_get_with_overrides(None, None)?
                };
                if !matches!(
                    snapshot["gatewayOriginator"].as_str(),
                    Some("q03-baseline" | "q03-get-patch-originator")
                ) {
                    return Err(format!("GET returned an unexpected originator: {snapshot}"));
                }
                if !matches!(
                    snapshot["sseKeepaliveIntervalMs"].as_u64(),
                    Some(15000 | 91)
                ) {
                    return Err(format!(
                        "GET returned an unexpected keepalive interval: {snapshot}"
                    ));
                }
                std::thread::yield_now();
            }
            Ok(())
        })();
        reader_finished
            .send(result)
            .expect("report reader completion");
    });
    let writers = [
        json!({"gatewayOriginator": "q03-get-patch-originator"}),
        json!({"sseKeepaliveIntervalMs": 91}),
    ]
    .map(|patch| {
        let barrier = Arc::clone(&barrier);
        let finished = finished.clone();
        std::thread::spawn(move || {
            barrier.wait();
            let result = service::app_settings_set(Some(&patch)).map(|_| ());
            finished.send(result).expect("report writer completion");
        })
    });
    drop(finished);
    barrier.wait();
    let results = (0..3)
        .map(|_| {
            completed
                .recv_timeout(Duration::from_secs(30))
                .expect("bounded concurrent GET/PATCH operation completed without a lock deadlock")
        })
        .collect::<Vec<_>>();
    reader.join().expect("reader thread did not panic");
    for writer in writers {
        writer.join().expect("writer thread did not panic");
    }
    for result in results {
        assert!(result.is_ok(), "concurrent GET/PATCH failed: {result:?}");
    }

    // Read runtime accessors before another GET can replay persisted values and
    // conceal an obsolete snapshot published by an earlier concurrent reader.
    assert_eq!(
        service::current_gateway_originator(),
        "q03-get-patch-originator"
    );
    assert_eq!(service::current_gateway_sse_keepalive_interval_ms(), 91);
    let persisted = rows(connection);
    assert_eq!(
        persisted[service::APP_SETTING_GATEWAY_ORIGINATOR_KEY].0,
        "q03-get-patch-originator"
    );
    assert_eq!(
        persisted[service::APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY].0,
        "91"
    );
    let snapshot = runtime_snapshot();
    assert_eq!(snapshot["gatewayOriginator"], "q03-get-patch-originator");
    assert_eq!(snapshot["sseKeepaliveIntervalMs"], 91);
}

fn standalone_gateway_setter_does_not_publish_on_failure(connection: &Connection) {
    baseline();
    let before = rows(connection);
    let runtime_before = runtime_snapshot();
    reject_writes_to(connection, service::APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY);
    assert!(service::set_gateway_route_strategy("ordered").is_err());
    assert_eq!(rows(connection), before);
    assert_eq!(
        runtime_snapshot()["routeStrategy"],
        runtime_before["routeStrategy"]
    );
    allow_writes(connection);
}

fn explicit_settings_and_environment_changes_do_not_overwrite_each_other(connection: &Connection) {
    baseline();
    save(json!({
        "envOverrides": {
            "CODEXMANAGER_ORIGINATOR": "q03-stale-environment",
            "CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS": "17"
        }
    }));
    let expected_version = "9.9.9-q03";
    let committed = save(json!({
        "gatewayOriginator": "q03-explicit-originator",
        "gatewayUserAgentVersion": expected_version,
        "routeStrategy": "ordered",
        "accountMaxInflight": 11,
        "quotaGuard": {"primaryMinRemainingPercent": 23},
        "backgroundTasks": {"usagePollingEnabled": false},
        "upstreamProxyUrl": "",
        "envOverrides": {
            "CODEXMANAGER_ORIGINATOR": "q03-conflicting-environment",
            "CODEXMANAGER_UPSTREAM_CONNECT_TIMEOUT_SECS": "37",
            "CODEXMANAGER_PROXY_LIST": "http://127.0.0.1:32991;http://127.0.0.1:32992"
        }
    }));
    assert_eq!(committed["gatewayOriginator"], "q03-explicit-originator");
    assert_eq!(
        committed["envOverrides"]["CODEXMANAGER_ORIGINATOR"],
        "q03-explicit-originator"
    );
    let responses = concurrent_patches(
        json!({"gatewayOriginator": "q03-concurrent-explicit"}),
        json!({"envOverrides": {"CODEXMANAGER_Q03_ENV_ROUND": "2"}}),
    );
    for response in responses {
        assert!(
            response.is_ok(),
            "explicit/environment concurrent patch failed: {response:?}"
        );
    }

    assert_eq!(
        service::current_gateway_originator(),
        "q03-concurrent-explicit"
    );
    assert_eq!(
        service::current_gateway_user_agent_version(),
        expected_version
    );
    assert_eq!(service::current_gateway_account_max_inflight(), 11);
    let snapshot = runtime_snapshot();
    assert_eq!(snapshot["routeStrategy"], "ordered");
    assert_eq!(
        snapshot["quotaGuard"]["primaryMinRemainingPercent"],
        json!(23.0)
    );
    assert_eq!(snapshot["backgroundTasks"]["usagePollingEnabled"], false);
    assert_eq!(snapshot["envOverrides"]["CODEXMANAGER_Q03_ENV_ROUND"], "2");
    assert_eq!(
        snapshot["envOverrides"]["CODEXMANAGER_ORIGINATOR"],
        "q03-concurrent-explicit"
    );
    let persisted = rows(connection);
    assert_eq!(
        persisted[service::APP_SETTING_GATEWAY_ORIGINATOR_KEY].0,
        "q03-concurrent-explicit"
    );
    let overrides: Value =
        serde_json::from_str(&persisted[service::APP_SETTING_ENV_OVERRIDES_KEY].0).unwrap();
    assert_eq!(
        overrides["CODEXMANAGER_ORIGINATOR"],
        "q03-concurrent-explicit"
    );
    assert_eq!(overrides["CODEXMANAGER_Q03_ENV_ROUND"], "2");

    // Return to an empty editor snapshot so later security tests do not retain
    // proxy configuration or a custom environment key from this scenario.
    save(json!({"envOverrides": {}}));
    save(json!({"envOverrides": {"CODEXMANAGER_Q03_ENV_ROUND": ""}}));
}

fn concurrent_web_auth_reads_never_observe_open_access() {
    save(json!({
        "webAccessPassword": "q03-concurrent-password",
        "webAuthMode": "password",
        "distributionEnabled": false
    }));
    let barrier = Arc::new(Barrier::new(2));
    let (finished, completed) = mpsc::channel();
    let reader_barrier = Arc::clone(&barrier);
    let reader_finished = finished.clone();
    let reader = std::thread::spawn(move || {
        let mut result = Ok(());
        for _ in 0..20 {
            reader_barrier.wait();
            for _ in 0..16 {
                let mode = service::current_web_auth_mode();
                if !matches!(mode.as_str(), "password" | "accounts") {
                    result = Err(format!(
                        "committed protected Web settings were read as {mode:?}"
                    ));
                }
                std::thread::yield_now();
            }
            reader_barrier.wait();
        }
        reader_finished
            .send(result)
            .expect("report Web auth reader completion");
    });
    let writer = std::thread::spawn(move || {
        let mut result = Ok(());
        for _ in 0..20 {
            barrier.wait();
            // Each committed target is protected. Reading its mode and hash in
            // separate snapshots could combine password mode with an empty hash.
            for patch in [
                json!({"webAuthMode": "accounts", "webAccessPassword": ""}),
                json!({
                    "webAuthMode": "password",
                    "webAccessPassword": "q03-concurrent-password"
                }),
            ] {
                if let Err(err) = service::app_settings_set(Some(&patch)) {
                    result = Err(err);
                }
                std::thread::yield_now();
            }
            // Complete all rounds even after an assertion failure so the other
            // worker can finish its bounded synchronization before we report it.
            barrier.wait();
        }
        finished
            .send(result)
            .expect("report Web auth writer completion");
    });
    let results = (0..2)
        .map(|_| {
            completed
                .recv_timeout(Duration::from_secs(30))
                .expect("bounded concurrent Web auth reader/writer completed without a deadlock")
        })
        .collect::<Vec<_>>();
    reader.join().expect("Web auth reader thread did not panic");
    writer.join().expect("Web auth writer thread did not panic");
    for result in results {
        assert!(
            result.is_ok(),
            "concurrent protected Web settings failed: {result:?}"
        );
    }
    assert_eq!(service::current_web_auth_mode(), "password");
    assert!(service::verify_web_access_password(
        "q03-concurrent-password"
    ));
}

fn final_web_security_target_and_lock_are_preserved(connection: &Connection) {
    save(json!({"webAuthMode": "none", "distributionEnabled": false, "webAccessPassword": ""}));
    assert_rejected_without_changes(
        connection,
        json!({
            "updateAutoCheck": false, "webAuthMode": "password"
        }),
    );
    let secured = save(json!({
        "webAccessPassword": "q03-temporary-password",
        "webAuthMode": "password",
        "distributionEnabled": false
    }));
    assert_eq!(secured["webAccessPasswordConfigured"], true);
    assert!(service::verify_web_access_password(
        "q03-temporary-password"
    ));
    assert_rejected_without_changes(
        connection,
        json!({
            "updateAutoCheck": false, "webAccessPassword": "", "webAuthMode": "password"
        }),
    );
    assert!(service::verify_web_access_password(
        "q03-temporary-password"
    ));
    assert!(!service::set_web_access_password(None).expect("legacy password-only clear succeeds"));
    assert!(!service::web_access_password_configured());
    assert_eq!(service::current_web_auth_mode(), "none");
    save(json!({
        "webAccessPassword": "q03-temporary-password",
        "webAuthMode": "password"
    }));
    let cleared = save(json!({"webAuthMode": "none", "webAccessPassword": ""}));
    assert_eq!(cleared["webAccessPasswordConfigured"], false);

    // Neither serialization order may produce distribution with an open Web UI.
    for _ in 0..6 {
        save(json!({"webAuthMode": "accounts", "distributionEnabled": false}));
        let responses = concurrent_patches(
            json!({"webAuthMode": "none"}),
            json!({"distributionEnabled": true}),
        );
        assert_eq!(responses.iter().filter(|result| result.is_ok()).count(), 1);
        let final_state = runtime_snapshot();
        assert!(
            final_state["distributionEnabled"] != true || final_state["webAuthMode"] == "accounts",
            "concurrent security patches bypassed the final-state invariant: {final_state}"
        );
    }

    save(json!({"webAuthMode": "accounts", "distributionEnabled": true}));
    service::create_app_user(service::AppUserCreateInput {
        username: "q03-lock-member".to_string(),
        password: "q03-member-password".to_string(),
        display_name: None,
        role: Some("member".to_string()),
        initial_balance_credit_micros: None,
    })
    .expect("create isolated billing-lock signal");
    let status = service::app_auth_status_value().unwrap();
    assert_eq!(status["billingModeLock"]["accountModeLocked"], true);
    assert_eq!(status["billingModeLock"]["distributionLocked"], true);
    let error = assert_rejected_without_changes(
        connection,
        json!({
            "updateAutoCheck": false,
            "webAccessPassword": "should-not-change-password",
            "webAuthMode": "none",
            "distributionEnabled": false
        }),
    );
    assert!(error.contains("account_billing_mode_locked"), "{error}");
    let error = assert_rejected_without_changes(
        connection,
        json!({
            "updateAutoCheck": false, "distributionEnabled": false
        }),
    );
    assert!(error.contains("distribution_mode_locked"), "{error}");
}

fn run_contract_suite(path: &Path) {
    let _runtime_env = ScopedEnv::clear(RUNTIME_ENV_KEYS);
    let _db = EnvGuard::set(
        "CODEXMANAGER_DB_PATH",
        path.to_str().expect("temporary path"),
    );
    service::initialize_storage_if_needed().expect("initialize disposable test database");
    let connection = Connection::open(path).expect("open test database for fault injection");
    mixed_invalid_fields_are_atomic(&connection);
    failed_persistence_rolls_back_every_field(&connection);
    concurrent_different_keys_do_not_rewrite_snapshots(&connection);
    concurrent_nested_patches_merge(&connection);
    concurrent_reads_do_not_overwrite_committed_patches(&connection);
    standalone_gateway_setter_does_not_publish_on_failure(&connection);
    explicit_settings_and_environment_changes_do_not_overwrite_each_other(&connection);
    concurrent_web_auth_reads_never_observe_open_access();
    final_web_security_target_and_lock_are_preserved(&connection);
}

#[test]
fn sqlite_settings_patch_atomicity() {
    if std::env::var(TEST_CHILD).as_deref() == Ok("1") {
        let _guard = test_env_guard();
        let directory =
            PathBuf::from(std::env::var_os(TEST_DIR).expect("parent-owned temp directory"));
        let _backend = EnvGuard::set("CODEXMANAGER_STORAGE_BACKEND", "sqlite");
        let _database_url = ScopedEnv::clear(&["CODEXMANAGER_DATABASE_URL"]);
        run_contract_suite(&directory.join("settings.db"));
        return;
    }
    run_isolated("sqlite_settings_patch_atomicity", "sqlite");
}

#[test]
fn seaorm_sqlite_settings_patch_atomicity() {
    if std::env::var(TEST_CHILD).as_deref() == Ok("1") {
        let _guard = test_env_guard();
        let directory =
            PathBuf::from(std::env::var_os(TEST_DIR).expect("parent-owned temp directory"));
        let path = directory.join("settings.db");
        let _backend = EnvGuard::set("CODEXMANAGER_STORAGE_BACKEND", "sqlite");
        let url = format!(
            "sqlite://{}?mode=rwc",
            path.to_string_lossy().replace('\\', "/")
        );
        let _url = EnvGuard::set("CODEXMANAGER_DATABASE_URL", &url);
        let _pool = EnvGuard::set("CODEXMANAGER_DB_MAX_CONNECTIONS", "2");
        run_contract_suite(&path);
        return;
    }

    run_isolated("seaorm_sqlite_settings_patch_atomicity", "seaorm-sqlite");
}

fn run_isolated(test: &str, backend: &str) {
    // The service pins pools/caches for the process lifetime. Child isolation
    // lets both adapters release Windows database handles before cleanup and
    // keeps all service caches and environment scoped to the disposable DB.
    let directory = TempDirectory::new(backend);
    let output = Command::new(std::env::current_exe().expect("current test executable"))
        .args(["--exact", test, "--nocapture"])
        .env(TEST_CHILD, "1")
        .env(TEST_DIR, &directory.0)
        .output()
        .expect("start isolated SeaORM integration test");
    assert!(
        output.status.success(),
        "{backend} service contract failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
