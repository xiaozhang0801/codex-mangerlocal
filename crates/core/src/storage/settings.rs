use rusqlite::params;

use super::Storage;

fn app_settings_list_sql() -> &'static str {
    "SELECT key, value
     FROM app_settings
     ORDER BY key ASC"
}

fn app_setting_value_by_key_sql() -> &'static str {
    "SELECT value
     FROM app_settings
     WHERE key = ?1
     LIMIT 1"
}

fn delete_app_setting_sql() -> &'static str {
    "DELETE FROM app_settings WHERE key = ?1"
}

fn set_app_setting_sql() -> &'static str {
    "INSERT INTO app_settings (key, value, updated_at)
     VALUES (?1, ?2, ?3)
     ON CONFLICT(key) DO UPDATE SET
       value = excluded.value,
       updated_at = excluded.updated_at"
}

impl Storage {
    /// Commit exactly the supplied setting changes in one transaction.
    /// `None` removes a setting; unrelated keys are never read or rewritten.
    pub fn save_app_settings(
        &self,
        changes: &[(String, Option<String>)],
        updated_at: i64,
    ) -> rusqlite::Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        for (key, value) in changes {
            match value {
                Some(value) => {
                    tx.execute(set_app_setting_sql(), params![key, value, updated_at])?;
                }
                None => {
                    tx.execute(delete_app_setting_sql(), [key])?;
                }
            }
        }
        tx.commit()
    }

    /// 函数 `list_app_settings`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn list_app_settings(&self) -> rusqlite::Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(app_settings_list_sql())?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    /// 函数 `get_app_setting`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key: 参数 key
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn get_app_setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        let mut stmt = self.conn.prepare(app_setting_value_by_key_sql())?;
        let mut rows = stmt.query([key])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(row.get(0)?));
        }
        Ok(None)
    }

    /// 函数 `set_app_setting`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key: 参数 key
    /// - value: 参数 value
    /// - updated_at: 参数 updated_at
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn set_app_setting(&self, key: &str, value: &str, updated_at: i64) -> rusqlite::Result<()> {
        self.conn
            .execute(set_app_setting_sql(), params![key, value, updated_at])?;
        Ok(())
    }

    /// 函数 `delete_app_setting`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key: 参数 key
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn delete_app_setting(&self, key: &str) -> rusqlite::Result<()> {
        self.conn.execute(delete_app_setting_sql(), [key])?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
