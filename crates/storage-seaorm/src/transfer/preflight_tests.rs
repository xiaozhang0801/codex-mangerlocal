use super::*;
use crate::{SeaOrmStorage, SettingsRepository};
use codexmanager_core::storage::StorageBackendKind;

async fn empty_target() -> SeaOrmStorage {
    let target = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
        .await
        .unwrap();
    target.migrate().await.unwrap();
    target
}

fn has(report: &TransferReport, code: &str) -> bool {
    report.failures.iter().any(|failure| failure.code == code)
}

fn write_settings_snapshot(path: &Path, rows: Vec<Vec<Cell>>) {
    let mut writer = BufWriter::new(File::create(path).unwrap());
    write_entry(&mut writer, &Entry::Header { version: VERSION }).unwrap();
    write_entry(
        &mut writer,
        &Entry::Table {
            name: "app_settings".into(),
            columns: vec!["key".into(), "value".into(), "updated_at".into()],
        },
    )
    .unwrap();
    let mut sum = [0; 32];
    for cells in &rows {
        add_hash(&mut sum, row_hash(cells).unwrap());
        write_entry(
            &mut writer,
            &Entry::Row {
                cells: cells.clone(),
            },
        )
        .unwrap();
    }
    write_entry(
        &mut writer,
        &Entry::EndTable {
            count: rows.len() as u64,
            sha256: hash_text(&sum),
        },
    )
    .unwrap();
    write_entry(&mut writer, &Entry::End { tables: 1 }).unwrap();
    writer.flush().unwrap();
}

#[tokio::test]
async fn dry_run_reports_types_conflicts_nonempty_and_reexecutes_without_mutation() {
    let (source, archive) = tests::paths();
    tests::legacy_fixture(&source);
    let expected = export_sqlite(&source, &archive).await.unwrap();
    let target = empty_target().await;
    for _ in 0..2 {
        let report = dry_run(&archive, target.connection()).await;
        assert!(
            report.success,
            "{}",
            serde_json::to_string(&report).unwrap()
        );
        assert!(!report.target_modified);
    }
    let original = std::fs::read(&archive).unwrap();
    write_settings_snapshot(
        &archive,
        vec![vec![
            Cell::Text("secret-key".into()),
            Cell::Text("private-token".into()),
            Cell::Text("invalid-time-secret".into()),
        ]],
    );
    let invalid = dry_run(&archive, target.connection()).await;
    assert!(has(&invalid, "source_value_type_or_range_invalid"));
    let output = serde_json::to_string(&invalid).unwrap();
    for secret in ["secret-key", "private-token", "invalid-time-secret"] {
        assert!(!output.contains(secret));
    }
    let duplicate = vec![
        Cell::Text("secret-key".into()),
        Cell::Text("private-token".into()),
        Cell::Integer(1),
    ];
    write_settings_snapshot(&archive, vec![duplicate.clone(), duplicate]);
    assert!(has(
        &dry_run(&archive, target.connection()).await,
        "source_unique_key_conflict"
    ));
    std::fs::write(&archive, &original).unwrap();
    assert_eq!(
        import_snapshot(&archive, target.connection())
            .await
            .unwrap(),
        expected
    );
    assert!(has(
        &dry_run(&archive, target.connection()).await,
        "target_not_empty"
    ));
    // The legacy adapter uses SQLite WAL. Opening a read-only SQLx handle can
    // checkpoint an already committed WAL into the main file, so a byte-for-
    // byte comparison would report a false mutation. Verify the source through
    // its public storage API instead: the fixture remains readable and its
    // committed setting survives every dry-run/import failure path.
    let source_db = codexmanager_core::storage::Storage::open(&source).unwrap();
    assert_eq!(
        source_db
            .get_app_setting("transfer_fixture")
            .unwrap()
            .as_deref(),
        Some("retained")
    );
    drop(source_db);
    assert_eq!(
        SettingsRepository::get(target.connection(), "transfer_fixture")
            .await
            .unwrap()
            .unwrap()
            .value,
        "retained"
    );
    std::fs::remove_file(archive).unwrap();
    // Keep the source path until the process exits: the legacy SQLite facade
    // owns a shared SQLx runtime and Windows can retain a WAL handle briefly
    // after the final `Storage` value is dropped.
}

#[tokio::test]
async fn dry_run_rejects_populated_unmapped_target_table() {
    let (source, archive) = tests::paths();
    tests::legacy_fixture(&source);
    export_sqlite(&source, &archive).await.unwrap();
    let target = empty_target().await;
    target
        .connection()
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE unmanaged_fixture (value TEXT NOT NULL)",
        ))
        .await
        .unwrap();
    target
        .connection()
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO unmanaged_fixture (value) VALUES ('opaque')",
        ))
        .await
        .unwrap();
    let report = dry_run(&archive, target.connection()).await;
    assert!(has(&report, "target_unmapped_populated_table"));
    assert!(report
        .tables
        .iter()
        .any(|table| !table.mapped && table.target_rows == Some(1)));
    assert!(!report.target_modified);
    let row = target
        .connection()
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS row_count FROM unmanaged_fixture",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<i64>("", "row_count").unwrap(), 1);
    std::fs::remove_file(archive).unwrap();
}

#[tokio::test]
async fn dry_run_missing_and_malformed_schema_remains_unmodified() {
    let (source, archive) = tests::paths();
    tests::legacy_fixture(&source);
    export_sqlite(&source, &archive).await.unwrap();
    let target = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
        .await
        .unwrap();
    let report = dry_run(&archive, target.connection()).await;
    assert!(has(&report, "target_table_missing"));
    let rows = target
        .connection()
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type='table'".to_owned(),
        ))
        .await
        .unwrap();
    assert!(rows.is_empty(), "preflight must not create schema");
    target.migrate().await.unwrap();
    target
        .connection()
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE app_settings RENAME COLUMN updated_at TO old_updated_at".to_owned(),
        ))
        .await
        .unwrap();
    target
        .connection()
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE app_settings ADD COLUMN updated_at TEXT".to_owned(),
        ))
        .await
        .unwrap();
    let report = dry_run(&archive, target.connection()).await;
    assert!(has(&report, "target_column_type_mismatch"));
    assert!(SettingsRepository::list(target.connection())
        .await
        .unwrap()
        .is_empty());
    std::fs::remove_file(archive).unwrap();
    std::fs::remove_file(source).unwrap();
}

#[tokio::test]
async fn failed_checksum_and_connection_preserve_source_and_allow_clean_restore() {
    const WAL_KEY: &str = "transfer_wal_restore_fixture";
    const WAL_VALUE: &str = "committed-wal-survives-failed-cutover";

    async fn target_lock_rows(db: &DatabaseConnection) -> Vec<(String, i64)> {
        db.query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, version FROM app_domain_locks ORDER BY id".to_owned(),
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| {
            (
                row.try_get("", "id").unwrap(),
                row.try_get("", "version").unwrap(),
            )
        })
        .collect()
    }

    let (source, archive) = tests::paths();
    tests::legacy_fixture(&source);
    // Dropping the legacy facade does not await its SQLx pool shutdown. Keep
    // one writer alive and pin an older read view so committed fixture rows
    // deterministically remain in WAL throughout the failed cutover.
    let source_connection = rusqlite::Connection::open(&source).unwrap();
    source_connection
        .execute_batch("PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    let checkpoint: (i64, i64, i64) = source_connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(
        checkpoint,
        (0, 0, 0),
        "fixture must start fully checkpointed"
    );
    let main_before = Sha256::digest(std::fs::read(&source).unwrap());
    let pinned_reader = rusqlite::Connection::open(&source).unwrap();
    pinned_reader.execute_batch("BEGIN DEFERRED").unwrap();
    let pinned_count = || {
        pinned_reader
            .query_row(
                "SELECT COUNT(*) FROM app_settings WHERE key = ?",
                [WAL_KEY],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
    };
    assert_eq!(pinned_count(), 0);
    source_connection
        .execute(
            "INSERT INTO app_settings (key, value, updated_at) VALUES (?, ?, 2)",
            [WAL_KEY, WAL_VALUE],
        )
        .unwrap();
    assert_eq!(pinned_count(), 0, "reader must retain its pre-commit view");
    let assert_source_intact = || {
        let source_db = codexmanager_core::storage::Storage::open(&source).unwrap();
        assert_eq!(
            source_db
                .get_app_setting("transfer_fixture")
                .unwrap()
                .as_deref(),
            Some("retained")
        );
        assert_eq!(
            source_db.get_app_setting(WAL_KEY).unwrap().as_deref(),
            Some(WAL_VALUE),
            "a new legacy reader must observe the committed WAL row"
        );
        let integrity: String = source_connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
        let main = std::fs::read(&source).unwrap();
        assert_eq!(Sha256::digest(&main), main_before);
        assert!(
            !main
                .windows(WAL_VALUE.len())
                .any(|bytes| bytes == WAL_VALUE.as_bytes()),
            "the committed sentinel must still be absent from the main file"
        );
        assert!(
            std::fs::metadata(source.with_extension("sqlite-wal"))
                .unwrap()
                .len()
                > 32,
            "the source must retain committed WAL frames"
        );
    };
    assert_source_intact();
    let expected = export_sqlite(&source, &archive).await.unwrap();
    let original = std::fs::read_to_string(&archive).unwrap();
    let target = empty_target().await;
    let initial_target_locks = target_lock_rows(target.connection()).await;
    std::fs::write(&archive, original.replace("retained", "tampered-secret")).unwrap();
    let failed_preflight = dry_run(&archive, target.connection()).await;
    assert!(has(&failed_preflight, "snapshot_checksum_mismatch"));
    assert!(!failed_preflight.target_modified);
    assert!(import_snapshot(&archive, target.connection())
        .await
        .unwrap_err()
        .to_string()
        .contains("checksum"));
    assert!(SettingsRepository::list(target.connection())
        .await
        .unwrap()
        .is_empty());
    std::fs::write(&archive, &original).unwrap();
    let missing = source.with_extension("missing-target.sqlite");
    let config = read_only_target_config(crate::StorageConfig {
        backend: StorageBackendKind::Sqlite,
        database_url: format!(
            "sqlite://{}?mode=rwc",
            missing.to_string_lossy().replace('\\', "/")
        ),
        max_connections: 1,
        acquire_timeout_ms: 50,
    })
    .unwrap();
    assert!(SeaOrmStorage::connect_with_config(config).await.is_err());
    assert!(!missing.exists());
    assert_source_intact();
    let after_failure_archive = source.with_extension("after-failure.jsonl");
    assert_eq!(
        export_sqlite(&source, &after_failure_archive)
            .await
            .unwrap(),
        expected,
        "failed checksum and connection paths must preserve every archived source row"
    );

    // SQLite's backup API reads a consistent SQL view including WAL. A raw
    // main-file copy here would omit WAL_VALUE despite its committed state.
    let restored = source.with_extension("restored.sqlite");
    let mut restored_connection = rusqlite::Connection::open(&restored).unwrap();
    rusqlite::backup::Backup::new(&source_connection, &mut restored_connection)
        .unwrap()
        .run_to_completion(64, std::time::Duration::ZERO, None)
        .unwrap();
    let restored_checkpoint: (i64, i64, i64) = restored_connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(restored_checkpoint, (0, 0, 0));
    let restored_db = codexmanager_core::storage::Storage::open(&restored).unwrap();
    assert_eq!(
        restored_db
            .get_app_setting("transfer_fixture")
            .unwrap()
            .as_deref(),
        Some("retained")
    );
    assert_eq!(
        restored_db.get_app_setting(WAL_KEY).unwrap().as_deref(),
        Some(WAL_VALUE)
    );
    let restored_integrity: String = restored_connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(restored_integrity, "ok");
    let restored_archive = source.with_extension("restored.jsonl");
    assert_eq!(
        export_sqlite(&restored, &restored_archive).await.unwrap(),
        expected,
        "the independently restored legacy database must contain the complete snapshot"
    );
    let clean_preflight = dry_run(&archive, target.connection()).await;
    assert!(clean_preflight.success);
    // Reports may include unmapped empty source tables with no target count.
    // Inspect every actual target table instead so failed imports cannot leave
    // rows in a mapped or an unexpected target table. Migration initializes
    // domain lock rows; those must retain their exact initial values.
    let target_tables = target
        .connection()
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
                .to_owned(),
        ))
        .await
        .unwrap();
    assert!(
        !target_tables.is_empty(),
        "the migrated target must have schema"
    );
    for table in target_tables {
        let name: String = table.try_get("", "name").unwrap();
        let count = target
            .connection()
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!("SELECT COUNT(*) AS row_count FROM {}", quote(&name)),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "row_count")
            .unwrap();
        if name == "app_domain_locks" {
            assert_eq!(count, initial_target_locks.len() as i64);
            assert_eq!(
                target_lock_rows(target.connection()).await,
                initial_target_locks,
                "failed cutover must preserve the initialized target locks"
            );
        } else {
            assert_eq!(
                count, 0,
                "failed cutover must leave target table {name} empty"
            );
        }
    }
    assert_eq!(
        import_snapshot(&archive, target.connection())
            .await
            .unwrap(),
        expected
    );
    for (key, value) in [("transfer_fixture", "retained"), (WAL_KEY, WAL_VALUE)] {
        assert_eq!(
            SettingsRepository::get(target.connection(), key)
                .await
                .unwrap()
                .unwrap()
                .value,
            value
        );
    }
    assert_source_intact();
    pinned_reader.execute_batch("ROLLBACK").unwrap();
    drop(restored_db);
    drop(restored_connection);
    drop(pinned_reader);
    drop(source_connection);
    for snapshot in [archive, after_failure_archive, restored_archive] {
        std::fs::remove_file(snapshot).unwrap();
    }
    // Keep both legacy database paths until process exit: dropping the facade
    // does not synchronously close the shared SQLx pool's Windows file handles.
}
async fn remote_dry_run(backend: StorageBackendKind, variable: &str) {
    let (source, archive) = tests::paths();
    tests::legacy_fixture(&source);
    export_sqlite(&source, &archive).await.unwrap();
    let target = SeaOrmStorage::connect(
        backend,
        &std::env::var(variable).expect("isolated new fixture URL"),
    )
    .await
    .unwrap();
    assert!(has(
        &dry_run(&archive, target.connection()).await,
        "target_table_missing"
    ));
    target.migrate().await.unwrap();
    for _ in 0..2 {
        let report = dry_run(&archive, target.connection()).await;
        assert!(
            report.success,
            "{}",
            serde_json::to_string(&report).unwrap()
        );
        assert!(!report.target_modified);
        assert!(
            SettingsRepository::list(target.connection())
                .await
                .unwrap()
                .is_empty(),
            "dry-run must not import source settings into the real target"
        );
    }
    import_snapshot(&archive, target.connection())
        .await
        .unwrap();
    assert!(has(
        &dry_run(&archive, target.connection()).await,
        "target_not_empty"
    ));
    assert_eq!(
        SettingsRepository::get(target.connection(), "transfer_fixture")
            .await
            .unwrap()
            .unwrap()
            .value,
        "retained"
    );
    // Await pool shutdown before deleting the fixture on Windows. Dropping the
    // synchronous compatibility facade only schedules SQLx handle cleanup.
    let file_url = url::Url::from_file_path(&source).unwrap();
    let source_db = SeaOrmStorage::connect(
        StorageBackendKind::Sqlite,
        &format!("sqlite://{}?mode=ro", file_url.path()),
    )
    .await
    .unwrap();
    assert_eq!(
        SettingsRepository::get(source_db.connection(), "transfer_fixture")
            .await
            .unwrap()
            .unwrap()
            .value,
        "retained"
    );
    source_db.connection().clone().close().await.unwrap();
    target.connection().clone().close().await.unwrap();
    std::fs::remove_file(archive).unwrap();
    std::fs::remove_file(source).unwrap();
}

#[cfg(feature = "mysql")]
#[tokio::test]
#[ignore = "requires a new isolated MySQL database"]
async fn mysql_dry_run_is_read_only_and_retries_before_import() {
    remote_dry_run(
        StorageBackendKind::Mysql,
        "CODEXMANAGER_TEST_DRYRUN_MYSQL_URL",
    )
    .await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires a new isolated PostgreSQL database"]
async fn postgres_dry_run_is_read_only_and_retries_before_import() {
    remote_dry_run(
        StorageBackendKind::Postgres,
        "CODEXMANAGER_TEST_DRYRUN_POSTGRES_URL",
    )
    .await;
}
