use sea_orm::entity::prelude::*;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, EntityTrait, QueryOrder, Set, TransactionTrait};

/// HTTP-independent domain value for a persisted application setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSetting {
    pub key: String,
    pub value: String,
    pub updated_at: i64,
}

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "app_settings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    key: String,
    #[sea_orm(column_type = "Text")]
    value: String,
    updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

pub struct SettingsRepository;

fn setting_upsert(setting: AppSetting) -> sea_orm::Insert<ActiveModel> {
    Entity::insert(ActiveModel {
        key: Set(setting.key),
        value: Set(setting.value),
        updated_at: Set(setting.updated_at),
    })
    .on_conflict(
        OnConflict::column(Column::Key)
            .update_columns([Column::Value, Column::UpdatedAt])
            .to_owned(),
    )
}

impl SettingsRepository {
    /// Save only the supplied changes, with the same commit boundary on every backend.
    /// `None` removes a setting. The transaction finishes before this returns.
    pub async fn save(
        db: &(impl ConnectionTrait + TransactionTrait),
        changes: &[(String, Option<String>)],
        updated_at: i64,
    ) -> Result<(), DbErr> {
        if changes.is_empty() {
            return Ok(());
        }
        let tx = db.begin().await?;
        for (key, value) in changes {
            let result = match value {
                Some(value) => {
                    Self::set(
                        &tx,
                        AppSetting {
                            key: key.clone(),
                            value: value.clone(),
                            updated_at,
                        },
                    )
                    .await
                }
                None => Self::delete(&tx, key).await.map(|_| ()),
            };
            if let Err(error) = result {
                tx.rollback().await.map_err(|rollback| {
                    DbErr::Custom(format!(
                        "save settings failed: {error}; rollback failed: {rollback}"
                    ))
                })?;
                return Err(error);
            }
        }
        tx.commit().await
    }

    pub async fn list(db: &impl ConnectionTrait) -> Result<Vec<AppSetting>, DbErr> {
        Entity::find()
            .order_by_asc(Column::Key)
            .all(db)
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    pub async fn get(db: &impl ConnectionTrait, key: &str) -> Result<Option<AppSetting>, DbErr> {
        Entity::find()
            .filter(Column::Key.eq(key))
            .one(db)
            .await
            .map(|row| row.map(Into::into))
    }

    pub async fn set(db: &impl ConnectionTrait, setting: AppSetting) -> Result<(), DbErr> {
        // A settings write does not need the inserted ID. In particular, MySQL
        // may report zero affected rows for an unchanged value, which is success.
        setting_upsert(setting)
            .exec_without_returning(db)
            .await
            .map(|_| ())
    }

    pub async fn delete(db: &impl ConnectionTrait, key: &str) -> Result<bool, DbErr> {
        Entity::delete_by_id(key)
            .exec(db)
            .await
            .map(|result| result.rows_affected > 0)
    }
}

impl From<Model> for AppSetting {
    fn from(model: Model) -> Self {
        Self {
            key: model.key,
            value: model.value,
            updated_at: model.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, DbBackend, QueryTrait, Statement};

    #[test]
    fn settings_mysql_upsert_changes_only_the_supplied_value_and_timestamp() {
        let statement = setting_upsert(AppSetting {
            key: "theme".into(),
            value: "dark".into(),
            updated_at: 7,
        })
        .build(DbBackend::MySql);
        assert_eq!(statement.sql,
            "INSERT INTO `app_settings` (`key`, `value`, `updated_at`) VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE `value` = VALUES(`value`), `updated_at` = VALUES(`updated_at`)");
        assert_eq!(
            statement.values.unwrap().0,
            vec!["theme".into(), "dark".into(), 7_i64.into()]
        );
    }

    #[test]
    fn settings_postgres_upsert_changes_only_the_supplied_value_and_timestamp() {
        let statement = setting_upsert(AppSetting {
            key: "theme".into(),
            value: "dark".into(),
            updated_at: 7,
        })
        .build(DbBackend::Postgres);
        assert_eq!(statement.sql,
            "INSERT INTO \"app_settings\" (\"key\", \"value\", \"updated_at\") VALUES ($1, $2, $3) ON CONFLICT (\"key\") DO UPDATE SET \"value\" = \"excluded\".\"value\", \"updated_at\" = \"excluded\".\"updated_at\"");
        assert_eq!(
            statement.values.unwrap().0,
            vec!["theme".into(), "dark".into(), 7_i64.into()]
        );
    }

    #[tokio::test]
    async fn settings_batch_rolls_back_updates_and_removals() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute(Statement::from_string(db.get_database_backend(),
            "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL CHECK(value != 'reject'), updated_at BIGINT NOT NULL)"))
            .await
            .unwrap();
        SettingsRepository::save(
            &db,
            &[
                ("changed".into(), Some("old".into())),
                ("removed".into(), Some("old".into())),
            ],
            1,
        )
        .await
        .unwrap();

        let result = SettingsRepository::save(
            &db,
            &[
                ("changed".into(), Some("new".into())),
                ("removed".into(), None),
                ("failed".into(), Some("reject".into())),
            ],
            2,
        )
        .await;

        assert!(result.is_err());
        let rows = SettingsRepository::list(&db).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|row| row.value == "old" && row.updated_at == 1));
        SettingsRepository::save(&db, &[("after".into(), Some("ok".into()))], 3)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn settings_repository_round_trip() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute(Statement::from_string(db.get_database_backend(), "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at BIGINT NOT NULL)")).await.unwrap();
        SettingsRepository::set(
            &db,
            AppSetting {
                key: "theme".into(),
                value: "dark".into(),
                updated_at: 7,
            },
        )
        .await
        .unwrap();
        SettingsRepository::set(
            &db,
            AppSetting {
                key: "theme".into(),
                value: "light".into(),
                updated_at: 8,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            SettingsRepository::get(&db, "theme")
                .await
                .unwrap()
                .unwrap()
                .value,
            "light"
        );
        assert_eq!(SettingsRepository::list(&db).await.unwrap().len(), 1);
        assert!(SettingsRepository::delete(&db, "theme").await.unwrap());
        assert!(SettingsRepository::get(&db, "theme")
            .await
            .unwrap()
            .is_none());
    }
}
