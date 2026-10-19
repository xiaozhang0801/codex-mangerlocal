use super::*;

async fn exercise_settings_batch_contract(storage: &dyn SettingsPluginsStore) {
    storage
        .save_settings(
            vec![
                ("theme".into(), Some("old".into())),
                ("removed".into(), Some("old".into())),
                ("untouched".into(), Some("keep".into())),
            ],
            1,
        )
        .await
        .unwrap();
    storage
        .save_settings(vec![("removed".into(), None)], 2)
        .await
        .unwrap();
    let (first, second) = tokio::join!(
        storage.save_settings(vec![("theme".into(), Some("new".into()))], 3),
        storage.save_settings(vec![("language".into(), Some("zh-CN".into()))], 4)
    );
    first.unwrap();
    second.unwrap();
    assert_eq!(
        storage.setting("theme".into()).await.unwrap().as_deref(),
        Some("new")
    );
    assert_eq!(
        storage.setting("language".into()).await.unwrap().as_deref(),
        Some("zh-CN")
    );
    assert_eq!(
        storage
            .setting("untouched".into())
            .await
            .unwrap()
            .as_deref(),
        Some("keep")
    );
    assert!(storage.setting("removed".into()).await.unwrap().is_none());
}

#[tokio::test]
async fn sqlite_domain_settings_batch_preserves_concurrent_fields() {
    let storage = Storage::open_in_memory().unwrap();
    storage.init().unwrap();
    exercise_settings_batch_contract(&SqliteDomainStorage::new(storage)).await;
}

#[tokio::test]
async fn seaorm_domain_settings_batch_preserves_concurrent_fields() {
    use sea_orm::{ConnectionTrait, Statement};
    let storage = crate::SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
        .await
        .unwrap();
    let db = storage.connection();
    db.execute(Statement::from_string(db.get_database_backend(),
        "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at BIGINT NOT NULL)"))
        .await
        .unwrap();
    exercise_settings_batch_contract(&storage).await;
}
