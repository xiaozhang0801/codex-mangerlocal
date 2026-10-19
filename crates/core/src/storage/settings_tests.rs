use super::*;

fn collect_query_plan(storage: &Storage, sql: &str) -> String {
    let mut stmt = storage.conn.prepare(sql).expect("prepare explain");
    let mut rows = stmt.query([]).expect("query explain");
    let mut plan = String::new();
    while let Some(row) = rows.next().expect("read explain row") {
        let detail: String = row.get(3).expect("plan detail");
        plan.push_str(&detail);
        plan.push('\n');
    }
    plan
}

#[test]
fn app_setting_lookup_uses_primary_key_index() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    let sql = app_setting_value_by_key_sql().replace("?1", "'gateway.routeStrategy'");
    let plan = collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {sql}"));

    assert!(
        plan.contains("sqlite_autoindex_app_settings_1"),
        "expected app setting lookup to use primary-key index, got {plan}"
    );
}

#[test]
fn app_settings_list_uses_primary_key_order_without_temp_sort() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    let plan = collect_query_plan(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", app_settings_list_sql()),
    );

    assert!(
        plan.contains("sqlite_autoindex_app_settings_1"),
        "expected app settings list to scan primary-key order, got {plan}"
    );
    assert!(
        !plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "expected app settings list to avoid temp ordering, got {plan}"
    );
}

#[test]
fn app_settings_batch_commits_updates_and_removals_only_for_supplied_keys() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    for (key, value) in [
        ("changed", "old"),
        ("removed", "old"),
        ("untouched", "keep"),
    ] {
        storage.set_app_setting(key, value, 1).unwrap();
    }

    storage
        .save_app_settings(
            &[
                ("changed".into(), Some("new".into())),
                ("removed".into(), None),
                ("added".into(), Some("value".into())),
            ],
            2,
        )
        .unwrap();

    assert_eq!(
        storage.get_app_setting("changed").unwrap().as_deref(),
        Some("new")
    );
    assert!(storage.get_app_setting("removed").unwrap().is_none());
    assert_eq!(
        storage.get_app_setting("added").unwrap().as_deref(),
        Some("value")
    );
    assert_eq!(
        storage.get_app_setting("untouched").unwrap().as_deref(),
        Some("keep")
    );
    let untouched_updated_at: i64 = storage
        .conn
        .query_row(
            "SELECT updated_at FROM app_settings WHERE key = 'untouched'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(untouched_updated_at, 1);
}

#[test]
fn app_settings_batch_rolls_back_updates_and_removals_when_later_write_fails() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.conn.execute_batch(
        "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL CHECK(value != 'reject'), updated_at BIGINT NOT NULL)",
    ).unwrap();
    storage.set_app_setting("changed", "old", 1).unwrap();
    storage.set_app_setting("removed", "old", 1).unwrap();

    let result = storage.save_app_settings(
        &[
            ("changed".into(), Some("new".into())),
            ("removed".into(), None),
            ("failed".into(), Some("reject".into())),
        ],
        2,
    );

    assert!(result.is_err());
    assert_eq!(
        storage.get_app_setting("changed").unwrap().as_deref(),
        Some("old")
    );
    assert_eq!(
        storage.get_app_setting("removed").unwrap().as_deref(),
        Some("old")
    );
    assert!(storage.get_app_setting("failed").unwrap().is_none());
    // A subsequent transaction must still be usable after the rollback.
    storage
        .save_app_settings(&[("after".into(), Some("ok".into()))], 3)
        .unwrap();
}

#[test]
fn concurrent_app_settings_batches_keep_different_fields() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    storage.set_app_setting("untouched", "keep", 1).unwrap();
    let first = storage.shared_handle();
    let second = storage.shared_handle();
    let start = std::sync::Arc::new(std::sync::Barrier::new(2));
    let first_start = start.clone();
    let first_write = std::thread::spawn(move || {
        first_start.wait();
        first.save_app_settings(&[("first".into(), Some("a".into()))], 2)
    });
    let second_write = std::thread::spawn(move || {
        start.wait();
        second.save_app_settings(&[("second".into(), Some("b".into()))], 3)
    });
    first_write.join().unwrap().unwrap();
    second_write.join().unwrap().unwrap();
    assert_eq!(
        storage.get_app_setting("first").unwrap().as_deref(),
        Some("a")
    );
    assert_eq!(
        storage.get_app_setting("second").unwrap().as_deref(),
        Some("b")
    );
    assert_eq!(
        storage.get_app_setting("untouched").unwrap().as_deref(),
        Some("keep")
    );
}
