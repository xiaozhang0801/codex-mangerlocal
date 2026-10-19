use super::*;
use codexmanager_core::storage::{now_ts, Storage};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
use std::sync::MutexGuard;
use std::time::{SystemTime, UNIX_EPOCH};

const CHILD_DIRECTORY: &str = "CODEXMANAGER_R02_ROUTE_SETTINGS_CHILD_DIRECTORY";

fn run_in_isolated_process() -> bool {
    const CHILD_TEST: &str = "CODEXMANAGER_R02_ROUTE_SETTINGS_CHILD_TEST";
    let current = std::thread::current();
    let test_name = current.name().expect("named test thread");
    if std::env::var(CHILD_TEST).as_deref() == Ok(test_name) {
        return false;
    }
    // The environment override baseline and storage pools intentionally live for
    // the process lifetime. A mutex and environment restoration cannot clear
    // their history, so each contract runs with fresh caches in its own process.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "codexmanager-route-settings-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).expect("create parent-owned settings directory");
    let output = Command::new(std::env::current_exe().expect("current test executable"))
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD_TEST, test_name)
        .env(CHILD_DIRECTORY, &directory)
        .output();
    // The exited child has released every SQLite/SQLx handle. Remove the tree
    // once here, including on assertion failure; no sleep or retry hides locks.
    let cleanup = std::fs::remove_dir_all(&directory);
    let output = output.unwrap_or_else(|error| {
        panic!(
            "could not start {test_name}: {error}; parent cleanup of {}: {cleanup:?}",
            directory.display()
        )
    });
    assert!(
        output.status.success() && cleanup.is_ok(),
        "{test_name} child status: {}; parent cleanup of {}: {cleanup:?}\n{}\n{}",
        output.status,
        directory.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

struct SettingsFixture {
    directory: PathBuf,
    previous_env: Vec<(OsString, OsString)>,
    _guard: MutexGuard<'static, ()>,
}

impl SettingsFixture {
    fn new() -> Self {
        let guard = crate::test_env_guard();
        static NEXT_FIXTURE: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);
        let parent_directory = PathBuf::from(
            std::env::var_os(CHILD_DIRECTORY).expect("parent-owned child settings directory"),
        );
        // A contract may exercise more than one fixture. All remain inside the
        // parent-owned root until child exit, when pool closure is guaranteed.
        let directory = parent_directory.join(format!(
            "fixture-{}",
            NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).expect("create isolated settings fixture");
        let fixture = Self {
            directory,
            previous_env: settings_process_env(),
            _guard: guard,
        };

        for (key, _) in &fixture.previous_env {
            std::env::remove_var(key);
        }
        std::env::set_var("CODEXMANAGER_DB_PATH", fixture.db_path());
        std::env::set_var("CODEXMANAGER_STORAGE_BACKEND", "sqlite");
        // Keep this fixture's pool small; parent cleanup waits for process exit.
        std::env::set_var("CODEXMANAGER_STORAGE_MAX_IDLE_CONNECTIONS", "0");
        crate::initialize_storage_if_needed().expect("initialize temporary SQLite settings");
        ensure_route_config_loaded();
        reload_from_env();
        clear_route_state_for_tests();
        fixture
    }

    fn db_path(&self) -> PathBuf {
        self.directory.join("settings.db")
    }

    fn save_strategy(&self, strategy: &str) {
        let storage = Storage::open(self.db_path()).expect("open temporary settings");
        storage
            .set_app_setting(
                crate::APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY,
                strategy,
                now_ts(),
            )
            .expect("persist route strategy");
    }

    fn saved_strategy(&self) -> Option<String> {
        Storage::open(self.db_path())
            .expect("open temporary settings")
            .get_app_setting(crate::APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY)
            .expect("read persisted strategy")
    }

    fn start_balanced_without_strategy_env(&self) {
        self.save_strategy("balanced");
        std::env::set_var(ROUTE_STRATEGY_ENV, "balanced");
        reload_from_env();
        std::env::remove_var(ROUTE_STRATEGY_ENV);
    }
}

impl Drop for SettingsFixture {
    fn drop(&mut self) {
        for (key, _) in settings_process_env() {
            std::env::remove_var(key);
        }
        for (key, value) in &self.previous_env {
            std::env::set_var(key, value);
        }
        crate::gateway::reload_runtime_config_from_env();
        crate::usage_refresh::reload_background_tasks_runtime_from_env();
        crate::usage_http::reload_usage_http_client_from_env();
        clear_route_state_for_tests();
    }
}

fn settings_process_env() -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(key, _)| {
            key.to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("CODEXMANAGER_")
        })
        .collect()
}

fn selected_account(key: &str) -> String {
    let mut candidates = super::tests::candidate_list();
    apply_route_strategy(&mut candidates, key, Some("gpt-5.3-codex"));
    candidates[0].0.id.clone()
}

#[test]
fn app_settings_reads_preserve_balanced_sequence_with_persisted_strategy() {
    if run_in_isolated_process() {
        return;
    }
    let fixture = SettingsFixture::new();
    fixture.start_balanced_without_strategy_env();
    let key = "settings-read-balanced";
    assert_eq!(selected_account(key), "acc-a");
    assert_eq!(selected_account(key), "acc-b");
    assert!(std::env::var_os(ROUTE_STRATEGY_ENV).is_none());

    // Natural reads also persist and then reload the default env snapshot. Do
    // not pre-populate those values: that path must preserve the cursor too.
    for expected in ["acc-c", "acc-a", "acc-b"] {
        let settings = crate::app_settings_get().expect("read settings during scheduling");
        assert_eq!(settings["routeStrategy"], "balanced");
        assert_eq!(selected_account(key), expected);
    }

    crate::author_content_get().expect("read author content during scheduling");
    assert_eq!(selected_account(key), "acc-c");
}

#[test]
fn app_settings_saves_and_alias_readback_preserve_balanced_sequence() {
    if run_in_isolated_process() {
        return;
    }
    let fixture = SettingsFixture::new();
    fixture.start_balanced_without_strategy_env();
    let key = "settings-save-balanced";
    assert_eq!(selected_account(key), "acc-a");
    assert_eq!(selected_account(key), "acc-b");

    for (strategy, after_save, after_read) in [
        ("balanced", "acc-c", "acc-a"),
        ("  ROUND_ROBIN  ", "acc-b", "acc-c"),
        ("rr", "acc-a", "acc-b"),
    ] {
        let saved = crate::app_settings_set(Some(&serde_json::json!({
            "routeStrategy": strategy,
        })))
        .expect("save equivalent route strategy");
        assert_eq!(saved["routeStrategy"], "balanced");
        assert_eq!(fixture.saved_strategy().as_deref(), Some("balanced"));
        assert_eq!(selected_account(key), after_save);

        let readback = crate::app_settings_get().expect("read back saved route strategy");
        assert_eq!(readback["routeStrategy"], "balanced");
        assert_eq!(selected_account(key), after_read);
    }
}

#[test]
fn startup_sync_loads_persisted_strategy_and_reapplying_keeps_cursor() {
    if run_in_isolated_process() {
        return;
    }
    let fixture = SettingsFixture::new();
    fixture.save_strategy("round-robin");
    assert!(std::env::var_os(ROUTE_STRATEGY_ENV).is_none());
    reload_from_env();
    assert_eq!(current_route_strategy(), "ordered");

    crate::sync_runtime_settings_from_storage();
    assert_eq!(current_route_strategy(), "balanced");
    let key = "settings-startup-balanced";
    assert_eq!(selected_account(key), "acc-a");
    assert_eq!(selected_account(key), "acc-b");

    // The gateway setter mirrors the strategy into the process environment.
    // Remove only that mirror so the second sync really reapplies storage.
    std::env::remove_var(ROUTE_STRATEGY_ENV);
    crate::sync_runtime_settings_from_storage();
    assert_eq!(current_route_strategy(), "balanced");
    assert_eq!(selected_account(key), "acc-c");
}

#[test]
fn startup_sync_preserves_explicit_strategy_env_over_persisted_strategy() {
    if run_in_isolated_process() {
        return;
    }
    for (explicit, persisted, canonical) in [
        ("priority", "balanced", "ordered"),
        ("RR", "ordered", "balanced"),
    ] {
        let fixture = SettingsFixture::new();
        fixture.save_strategy(persisted);
        std::env::set_var(ROUTE_STRATEGY_ENV, explicit);
        reload_from_env();

        crate::sync_runtime_settings_from_storage();
        assert_eq!(current_route_strategy(), canonical);
        assert_eq!(std::env::var(ROUTE_STRATEGY_ENV).unwrap(), explicit);
        assert_eq!(fixture.saved_strategy().as_deref(), Some(persisted));

        let snapshot = crate::app_settings_get().expect("read explicit strategy override");
        assert_eq!(snapshot["routeStrategy"], canonical);
        assert_eq!(current_route_strategy(), canonical);
    }
}

#[test]
fn combined_reads_unrelated_patch_and_equivalent_strategies_preserve_next_account() {
    if run_in_isolated_process() {
        return;
    }
    let fixture = SettingsFixture::new();
    fixture.start_balanced_without_strategy_env();
    let key = "r02-combined-settings-sequence";
    assert_eq!(selected_account(key), "acc-a");
    assert_eq!(selected_account(key), "acc-b");

    let read = crate::app_settings_get().expect("read settings between selections");
    assert_eq!(read["routeStrategy"], "balanced");
    let saved = crate::app_settings_set(Some(&json!({"updateAutoCheck": false})))
        .expect("save unrelated UI setting");
    assert_eq!(saved["updateAutoCheck"], false);
    for equivalent in ["balanced", "  ROUND_ROBIN  ", "round-robin", "rr"] {
        let saved = crate::app_settings_set(Some(&json!({"routeStrategy": equivalent})))
            .expect("save equivalent route strategy");
        assert_eq!(saved["routeStrategy"], "balanced");
        assert_eq!(fixture.saved_strategy().as_deref(), Some("balanced"));
        assert_eq!(
            crate::set_gateway_route_strategy(equivalent).expect("apply legacy route setter"),
            "balanced"
        );
        crate::app_settings_get().expect("read back equivalent strategy");
    }
    crate::author_content_get().expect("read unrelated author settings");
    assert_eq!(selected_account(key), "acc-c");
    assert_eq!(selected_account(key), "acc-a");

    let ordered = crate::app_settings_set(Some(&json!({"routeStrategy": "priority"})))
        .expect("actually switch to ordered");
    assert_eq!(ordered["routeStrategy"], "ordered");
    assert_eq!(selected_account(key), "acc-a");
    let balanced = crate::app_settings_set(Some(&json!({"routeStrategy": "rr"})))
        .expect("actually switch back to balanced");
    assert_eq!(balanced["routeStrategy"], "balanced");
    assert_eq!(
        selected_account(key),
        "acc-a",
        "a real strategy change resets the cursor"
    );
    assert_eq!(selected_account(key), "acc-b");
}

#[test]
fn unrelated_environment_patch_preserves_runtime_balanced_cursor_without_strategy_env() {
    if run_in_isolated_process() {
        return;
    }
    let fixture = SettingsFixture::new();
    fixture.start_balanced_without_strategy_env();
    let key = "r02-unrelated-environment";
    assert_eq!(selected_account(key), "acc-a");
    assert_eq!(selected_account(key), "acc-b");
    assert!(std::env::var_os(ROUTE_STRATEGY_ENV).is_none());
    let saved = crate::app_settings_set(Some(&json!({
        "envOverrides": {"CODEXMANAGER_R02_UNRELATED_ROUTE_TEST": "changed"}
    })))
    .expect("save unrelated environment value");
    assert_eq!(saved["routeStrategy"], "balanced");
    assert_eq!(
        std::env::var("CODEXMANAGER_R02_UNRELATED_ROUTE_TEST").unwrap(),
        "changed"
    );
    assert_eq!(fixture.saved_strategy().as_deref(), Some("balanced"));
    assert_eq!(
        selected_account(key),
        "acc-c",
        "environment reload must not temporarily switch strategy"
    );
}

#[test]
fn storage_sync_of_unrelated_environment_preserves_balanced_cursor() {
    if run_in_isolated_process() {
        return;
    }
    let fixture = SettingsFixture::new();
    fixture.start_balanced_without_strategy_env();
    Storage::open(fixture.db_path())
        .expect("open disposable environment settings")
        .set_app_setting(
            crate::APP_SETTING_ENV_OVERRIDES_KEY,
            &json!({"CODEXMANAGER_R02_SYNC_ROUTE_TEST": "persisted"}).to_string(),
            now_ts(),
        )
        .expect("persist an unapplied unrelated environment override");
    let key = "r02-storage-sync-environment";
    assert_eq!(selected_account(key), "acc-a");
    assert_eq!(selected_account(key), "acc-b");
    assert!(std::env::var_os(ROUTE_STRATEGY_ENV).is_none());
    assert!(std::env::var_os("CODEXMANAGER_R02_SYNC_ROUTE_TEST").is_none());

    let read =
        crate::app_settings_get().expect("sync persisted environment while reading settings");
    assert_eq!(read["routeStrategy"], "balanced");
    assert_eq!(
        std::env::var("CODEXMANAGER_R02_SYNC_ROUTE_TEST").unwrap(),
        "persisted"
    );
    assert_eq!(fixture.saved_strategy().as_deref(), Some("balanced"));
    assert_eq!(
        selected_account(key),
        "acc-c",
        "storage synchronization must not temporarily switch away from the unchanged strategy"
    );
}

#[test]
fn blank_strategy_env_preserves_cursor_until_persisted_strategy_actually_changes() {
    if run_in_isolated_process() {
        return;
    }
    let fixture = SettingsFixture::new();
    fixture.start_balanced_without_strategy_env();
    std::env::set_var(ROUTE_STRATEGY_ENV, " \t ");
    Storage::open(fixture.db_path())
        .expect("open disposable blank-env settings")
        .set_app_setting(
            crate::APP_SETTING_ENV_OVERRIDES_KEY,
            &json!({"CODEXMANAGER_R02_BLANK_ENV_ROUTE_TEST": "persisted"}).to_string(),
            now_ts(),
        )
        .expect("persist unrelated environment override");
    let key = "r02-blank-env-storage-sync";
    assert_eq!(selected_account(key), "acc-a");
    assert_eq!(selected_account(key), "acc-b");

    let read = crate::app_settings_get().expect("sync settings with a blank strategy environment");
    assert_eq!(read["routeStrategy"], "balanced");
    assert_eq!(
        std::env::var("CODEXMANAGER_R02_BLANK_ENV_ROUTE_TEST").unwrap(),
        "persisted"
    );
    assert_eq!(selected_account(key), "acc-c");
    // Leave the balanced cursor at B, so later A proves a reset rather than
    // coinciding with a natural wraparound after C.
    assert_eq!(selected_account(key), "acc-a");

    fixture.save_strategy("ordered");
    // A successful strategy publication mirrors its value into the process.
    // Restore a blank value so storage is authoritative for each genuine change.
    std::env::set_var(ROUTE_STRATEGY_ENV, " \t ");
    let read = crate::app_settings_get().expect("sync a genuinely changed ordered strategy");
    assert_eq!(read["routeStrategy"], "ordered");
    assert_eq!(selected_account(key), "acc-a");

    fixture.save_strategy("balanced");
    std::env::set_var(ROUTE_STRATEGY_ENV, " \t ");
    let read = crate::app_settings_get().expect("sync a genuinely changed balanced strategy");
    assert_eq!(read["routeStrategy"], "balanced");
    assert_eq!(
        selected_account(key),
        "acc-a",
        "actual persisted strategy changes reset the cursor"
    );
    assert_eq!(selected_account(key), "acc-b");
}

type SettingRows = BTreeMap<String, (String, i64)>;

fn saved_rows(connection: &Connection) -> SettingRows {
    let mut query = connection
        .prepare("SELECT key, value, updated_at FROM app_settings ORDER BY key")
        .expect("prepare settings snapshot");
    query
        .query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))
        .expect("read settings snapshot")
        .collect::<Result<SettingRows, _>>()
        .expect("decode settings snapshot")
}

fn runtime_snapshot_without_reload() -> Value {
    // A GET could repair an early publication and conceal a failed guarantee.
    crate::app_settings_set(Some(&json!({})))
        .expect("read post-patch snapshot without runtime reload")
}

fn assert_rejected_patch_preserves_settings_and_cursor(
    connection: &Connection,
    key: &str,
    patch: Value,
) -> String {
    let rows_before = saved_rows(connection);
    let runtime_before = runtime_snapshot_without_reload();
    let env_before = settings_process_env()
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    assert_eq!(selected_account(key), "acc-a");
    assert_eq!(selected_account(key), "acc-b");
    let error = crate::app_settings_set(Some(&patch)).expect_err("reject the complete patch");
    assert_eq!(
        saved_rows(connection),
        rows_before,
        "failed patch changed persisted rows"
    );
    assert_eq!(
        runtime_snapshot_without_reload(),
        runtime_before,
        "failed patch published runtime fields"
    );
    assert_eq!(
        settings_process_env()
            .into_iter()
            .collect::<BTreeMap<_, _>>(),
        env_before,
        "failed patch changed process environment"
    );
    assert_eq!(
        selected_account(key),
        "acc-c",
        "failed patch must not reset scheduling"
    );
    error
}

#[test]
fn mixed_invalid_patch_preserves_database_runtime_and_balanced_sequence() {
    if run_in_isolated_process() {
        return;
    }
    let fixture = SettingsFixture::new();
    fixture.start_balanced_without_strategy_env();
    let connection =
        Connection::open(fixture.db_path()).expect("open disposable settings database");
    for (index, patch) in [
        json!({"updateAutoCheck": false, "routeStrategy": "invalid"}),
        json!({"updateAutoCheck": false, "routeStrategy": "ordered", "gatewayUserAgent": "invalid\r\nheader"}),
    ].into_iter().enumerate() {
        assert_rejected_patch_preserves_settings_and_cursor(
            &connection,
            &format!("r02-invalid-mixed-{index}"),
            patch,
        );
    }
    drop(connection);
}

#[test]
fn committed_settings_wal_backup_and_rejected_patch_preserve_balanced_sequence() {
    if run_in_isolated_process() {
        return;
    }
    const WAL_ORIGINATOR: &str = "s01-committed-settings-live-only-in-wal";
    let fixture = SettingsFixture::new();
    fixture.start_balanced_without_strategy_env();
    crate::app_settings_set(Some(&json!({
        "updateAutoCheck": true,
        "gatewayOriginator": "s01-before-wal-commit",
    })))
    .expect("seed settings before the WAL commit");
    let connection = Connection::open(fixture.db_path()).expect("open disposable WAL settings");
    connection
        .execute_batch("PRAGMA wal_autocheckpoint=0;")
        .expect("retain committed WAL frames");
    let checkpoint: (i64, i64, i64) = connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .expect("checkpoint the pre-commit database");
    assert_eq!(checkpoint, (0, 0, 0));
    let rows_before = saved_rows(&connection);
    let main_before = std::fs::read(fixture.db_path()).expect("read checkpointed settings file");
    let pinned_reader = Connection::open(fixture.db_path()).expect("open old settings reader");
    pinned_reader
        .execute_batch("BEGIN DEFERRED")
        .expect("pin the pre-commit settings view");
    assert_eq!(saved_rows(&pinned_reader), rows_before);

    let key = "s01-wal-backup-settings-sequence";
    assert_eq!(selected_account(key), "acc-a");
    assert_eq!(selected_account(key), "acc-b");
    let committed = crate::app_settings_set(Some(&json!({
        "updateAutoCheck": false,
        "gatewayOriginator": WAL_ORIGINATOR,
        "routeStrategy": "  ROUND_ROBIN  ",
    })))
    .expect("commit a complete settings patch while an old WAL reader is active");
    assert_eq!(committed["updateAutoCheck"], false);
    assert_eq!(committed["gatewayOriginator"], WAL_ORIGINATOR);
    assert_eq!(committed["routeStrategy"], "balanced");
    let rows_committed = saved_rows(&connection);
    assert_ne!(rows_committed, rows_before);
    for (setting, value) in [
        (crate::APP_SETTING_UPDATE_AUTO_CHECK_KEY, "0"),
        (crate::APP_SETTING_GATEWAY_ORIGINATOR_KEY, WAL_ORIGINATOR),
        (crate::APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY, "balanced"),
    ] {
        assert_eq!(rows_committed[setting].0, value);
    }
    assert_eq!(saved_rows(&pinned_reader), rows_before);
    let main_after = std::fs::read(fixture.db_path()).expect("read live settings main file");
    assert_eq!(
        main_after, main_before,
        "old reader must prevent checkpointing"
    );
    assert!(
        !main_after
            .windows(WAL_ORIGINATOR.len())
            .any(|bytes| bytes == WAL_ORIGINATOR.as_bytes()),
        "the committed setting must still be absent from the main file"
    );
    assert!(
        std::fs::metadata(fixture.db_path().with_extension("db-wal"))
            .expect("inspect committed WAL")
            .len()
            > 32
    );
    assert_eq!(
        selected_account(key),
        "acc-c",
        "alias commit reset the cursor"
    );

    assert_eq!(selected_account(key), "acc-a");
    assert_eq!(selected_account(key), "acc-b");
    let mut snapshot = Connection::open(fixture.directory.join("settings-backup.db"))
        .expect("open independent settings snapshot");
    rusqlite::backup::Backup::new(&connection, &mut snapshot)
        .expect("create settings Backup")
        .run_to_completion(64, std::time::Duration::ZERO, None)
        .expect("capture the committed WAL view");
    assert_eq!(saved_rows(&snapshot), rows_committed);
    assert_eq!(saved_rows(&connection), rows_committed);
    assert_eq!(selected_account(key), "acc-c", "backup reset the cursor");

    assert_rejected_patch_preserves_settings_and_cursor(
        &connection,
        key,
        json!({
            "updateAutoCheck": true,
            "gatewayOriginator": "s01-rejected-must-not-publish",
            "routeStrategy": "ordered",
            "gatewayUserAgent": "invalid\r\nheader",
        }),
    );
    assert_eq!(saved_rows(&connection), rows_committed);

    // Move the live database beyond the backup. Restoring must recover the
    // committed snapshot, including timestamps, rather than read the live path.
    assert_eq!(selected_account(key), "acc-a");
    assert_eq!(selected_account(key), "acc-b");
    crate::app_settings_set(Some(&json!({
        "updateAutoCheck": true,
        "gatewayOriginator": "s01-live-after-backup",
    })))
    .expect("commit later settings independently of the snapshot");
    let rows_later = saved_rows(&connection);
    assert_ne!(rows_later, rows_committed);
    assert_eq!(rows_later[crate::APP_SETTING_UPDATE_AUTO_CHECK_KEY].0, "1");
    assert_eq!(
        rows_later[crate::APP_SETTING_GATEWAY_ORIGINATOR_KEY].0,
        "s01-live-after-backup"
    );
    assert_eq!(selected_account(key), "acc-c");
    let runtime_later = runtime_snapshot_without_reload();

    assert_eq!(selected_account(key), "acc-a");
    assert_eq!(selected_account(key), "acc-b");
    let restored_path = fixture.directory.join("settings-restored.db");
    let mut restored = Connection::open(&restored_path).expect("open independent restore target");
    rusqlite::backup::Backup::new(&snapshot, &mut restored)
        .expect("restore settings Backup")
        .run_to_completion(64, std::time::Duration::ZERO, None)
        .expect("restore the complete committed settings snapshot");
    assert_eq!(saved_rows(&restored), rows_committed);
    let integrity: String = restored
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("check restored settings integrity");
    assert_eq!(integrity, "ok");
    let restored_storage = Storage::open(&restored_path).expect("reopen restored settings facade");
    assert_eq!(
        restored_storage
            .get_app_setting(crate::APP_SETTING_GATEWAY_ORIGINATOR_KEY)
            .expect("read restored committed originator")
            .as_deref(),
        Some(WAL_ORIGINATOR)
    );
    assert_eq!(saved_rows(&connection), rows_later);
    assert_eq!(saved_rows(&snapshot), rows_committed);
    assert_eq!(saved_rows(&pinned_reader), rows_before);
    assert_eq!(runtime_snapshot_without_reload(), runtime_later);
    assert_eq!(
        selected_account(key),
        "acc-c",
        "restore reset the live cursor"
    );
    pinned_reader
        .execute_batch("ROLLBACK")
        .expect("release the old settings view");
}

#[test]
fn transaction_failure_after_earlier_writes_preserves_balanced_sequence() {
    if run_in_isolated_process() {
        return;
    }
    let fixture = SettingsFixture::new();
    fixture.start_balanced_without_strategy_env();
    let connection =
        Connection::open(fixture.db_path()).expect("open disposable fault injection database");
    let failed_key = crate::APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY;
    assert!(crate::APP_SETTING_UPDATE_AUTO_CHECK_KEY < failed_key);
    assert!(failed_key < crate::APP_SETTING_GATEWAY_SSE_KEEPALIVE_ENABLED_KEY);
    for event in ["INSERT", "UPDATE"] {
        connection.execute(
            &format!("CREATE TRIGGER r02_reject_{event} BEFORE {event} ON app_settings WHEN NEW.key = '{failed_key}' BEGIN SELECT RAISE(ABORT, 'r02 injected middle settings failure'); END"),
            [],
        ).expect("install transaction-middle fault");
    }
    let error = assert_rejected_patch_preserves_settings_and_cursor(
        &connection,
        "r02-transaction-failure",
        json!({
            "updateAutoCheck": false,
            "gatewayOriginator": "r02-must-not-publish",
            "routeStrategy": "ordered",
            "sseKeepaliveEnabled": false
        }),
    );
    assert!(
        error.contains("r02 injected middle settings failure"),
        "{error}"
    );
    for event in ["INSERT", "UPDATE"] {
        connection
            .execute(&format!("DROP TRIGGER r02_reject_{event}"), [])
            .expect("remove injected fault");
    }
    drop(connection);
}
