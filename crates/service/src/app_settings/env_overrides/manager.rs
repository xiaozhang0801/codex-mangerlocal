use std::collections::{BTreeMap, HashMap};

#[derive(Debug)]
pub(crate) struct PreparedEnvOverrides {
    pub(crate) previous: BTreeMap<String, String>,
    pub(crate) next: BTreeMap<String, String>,
    pub(crate) raw: String,
}

impl PreparedEnvOverrides {
    pub(crate) fn publish(self) -> BTreeMap<String, String> {
        publish_env_overrides(self)
    }

    /// Values the process will expose after this delta publishes. An empty value
    /// explicitly represents an absent variable; consumers must not fall back to
    /// the current process value when the returned map contains that key.
    pub(crate) fn effective_process_env(&self) -> BTreeMap<String, String> {
        self.previous
            .keys()
            .chain(self.next.keys())
            .map(|key| {
                let value = if self.previous.get(key) == self.next.get(key) {
                    // Unchanged overrides are deliberately not replayed.
                    super::normalize_optional_text(std::env::var(key).ok().as_deref())
                } else {
                    self.next
                        .get(key)
                        .filter(|value| !value.trim().is_empty())
                        .cloned()
                        .or_else(|| super::process::env_override_original_process_value(key))
                };
                (key.clone(), value.unwrap_or_default())
            })
            .collect()
    }

    /// An explicit settings field wins over an environment editor value in the
    /// same transaction. Keep any existing overlapping override in agreement so
    /// a later environment patch or restart cannot restore a stale value.
    pub(crate) fn align_runtime_settings(
        &mut self,
        settings: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        use super::super::shared::*;
        for (setting, env_key) in [
            (
                APP_SETTING_GATEWAY_ORIGINATOR_KEY,
                "CODEXMANAGER_ORIGINATOR",
            ),
            (
                APP_SETTING_GATEWAY_UPSTREAM_TOTAL_TIMEOUT_MS_KEY,
                "CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS",
            ),
            (
                APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY,
                "CODEXMANAGER_FREE_ACCOUNT_MAX_MODEL",
            ),
            (
                APP_SETTING_GATEWAY_MODEL_FORWARD_RULES_KEY,
                "CODEXMANAGER_MODEL_FORWARD_RULES",
            ),
            (
                APP_SETTING_GATEWAY_COMPACT_MODEL_FORWARD_RULES_KEY,
                "CODEXMANAGER_COMPACT_MODEL_FORWARD_RULES",
            ),
            (
                APP_SETTING_GATEWAY_RESIDENCY_REQUIREMENT_KEY,
                "CODEXMANAGER_RESIDENCY_REQUIREMENT",
            ),
            (
                APP_SETTING_GATEWAY_UPSTREAM_PROXY_BYPASS_HOSTS_KEY,
                "CODEXMANAGER_UPSTREAM_PROXY_BYPASS_HOSTS",
            ),
        ] {
            if let Some(value) = settings.get(setting) {
                self.align_env_value(env_key, value);
            }
        }
        if let Some(raw) = settings.get(APP_SETTING_GATEWAY_QUOTA_GUARD_KEY) {
            let config: crate::gateway::QuotaGuardConfig = serde_json::from_str(raw)
                .map_err(|err| format!("parse prepared quota guard failed: {err}"))?;
            for (env_key, value) in [
                (
                    "CODEXMANAGER_QUOTA_GUARD_ENABLED",
                    config.enabled.to_string(),
                ),
                (
                    "CODEXMANAGER_QUOTA_GUARD_5H_MIN_REMAINING_PERCENT",
                    config.primary_min_remaining_percent.to_string(),
                ),
                (
                    "CODEXMANAGER_QUOTA_GUARD_WEEKLY_MIN_REMAINING_PERCENT",
                    config.secondary_min_remaining_percent.to_string(),
                ),
                (
                    "CODEXMANAGER_QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK",
                    config.allow_all_low_quota_fallback.to_string(),
                ),
            ] {
                self.align_env_value(env_key, &value);
            }
        }
        self.raw = super::snapshot::serialize_env_overrides_value(&self.next)?;
        Ok(())
    }

    fn align_env_value(&mut self, key: &str, value: &str) {
        if super::catalog::is_env_override_catalog_key(key) || self.next.contains_key(key) {
            self.next.insert(key.to_string(), value.to_string());
        }
    }
}

/// Synchronize only an already persisted override map. A regular settings write
/// must not create the environment editor's full default snapshot as a side effect.
pub(crate) fn prepare_env_overrides_for_settings(
    settings: &BTreeMap<String, String>,
) -> Result<Option<PreparedEnvOverrides>, String> {
    if super::get_persisted_app_setting(super::APP_SETTING_ENV_OVERRIDES_KEY).is_none() {
        return Ok(None);
    }
    let previous = super::snapshot::persisted_env_overrides_only();
    let mut prepared = PreparedEnvOverrides {
        raw: String::new(),
        next: previous.clone(),
        previous,
    };
    prepared.align_runtime_settings(settings)?;
    if prepared.previous == prepared.next {
        return Ok(None);
    }
    for (key, value) in &prepared.next {
        if value.contains('\0') {
            return Err(format!("{key} environment variable value contains NUL"));
        }
    }
    Ok(Some(prepared))
}

/// 函数 `set_env_overrides`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn set_env_overrides(
    overrides: HashMap<String, String>,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let raw = crate::app_settings::api::apply_single_setting_patch(
        serde_json::json!({"envOverrides": overrides}),
        super::APP_SETTING_ENV_OVERRIDES_KEY,
    )?;
    serde_json::from_str(&raw).map_err(|err| err.to_string())
}

/// Build the persisted value before changing the process environment.
/// Callers that prepare and commit several settings hold the settings write lock
/// until publication so another patch cannot merge against a stale snapshot.
pub(crate) fn prepare_env_overrides(
    overrides: HashMap<String, String>,
) -> Result<PreparedEnvOverrides, String> {
    let previous = super::snapshot::current_env_overrides();
    let patch = super::normalize::normalize_env_overrides_patch(overrides)?;
    let mut next = if patch.is_empty() {
        super::snapshot::env_override_default_snapshot()
    } else {
        previous.clone()
    };

    for (key, value) in patch {
        if let Some(value) = value {
            next.insert(key, value);
        } else if super::catalog::is_env_override_catalog_key(&key) {
            next.insert(
                key.clone(),
                super::snapshot::env_override_default_value(&key),
            );
        } else {
            next.remove(&key);
        }
    }

    for item in super::catalog::editable_env_override_catalog() {
        next.entry(item.key.to_string())
            .or_insert_with(|| super::snapshot::env_override_default_value(item.key));
    }

    // set_var cannot accept NUL. Reject it before the transaction commits,
    // including values loaded from an older persisted snapshot.
    for (key, value) in &next {
        if value.contains('\0') {
            return Err(format!("{key} environment variable value contains NUL"));
        }
    }
    let raw = super::snapshot::serialize_env_overrides_value(&next)?;
    Ok(PreparedEnvOverrides {
        previous,
        next,
        raw,
    })
}

/// Publish only after the prepared JSON has committed successfully.
pub(crate) fn publish_env_overrides(prepared: PreparedEnvOverrides) -> BTreeMap<String, String> {
    super::process::publish_env_override_changes(&prepared.previous, &prepared.next);
    prepared.next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::shared::*;

    #[test]
    fn explicit_settings_align_catalog_and_existing_custom_overrides() {
        let previous = BTreeMap::from([
            ("CODEXMANAGER_ORIGINATOR".to_string(), "old".to_string()),
            (
                "CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS".to_string(),
                "0".to_string(),
            ),
            (
                "CODEXMANAGER_MODEL_FORWARD_RULES".to_string(),
                "old=model".to_string(),
            ),
            ("CODEXMANAGER_Q03_UNRELATED".to_string(), "keep".to_string()),
        ]);
        let mut prepared = PreparedEnvOverrides {
            next: previous.clone(),
            previous,
            raw: String::new(),
        };
        prepared
            .align_runtime_settings(&BTreeMap::from([
                (
                    APP_SETTING_GATEWAY_ORIGINATOR_KEY.to_string(),
                    "new".to_string(),
                ),
                (
                    APP_SETTING_GATEWAY_UPSTREAM_TOTAL_TIMEOUT_MS_KEY.to_string(),
                    "1234".to_string(),
                ),
                (
                    APP_SETTING_GATEWAY_MODEL_FORWARD_RULES_KEY.to_string(),
                    "new=model".to_string(),
                ),
            ]))
            .expect("align prepared settings");

        assert_eq!(prepared.next["CODEXMANAGER_ORIGINATOR"], "new");
        assert_eq!(
            prepared.next["CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS"],
            "1234"
        );
        assert_eq!(
            prepared.next["CODEXMANAGER_MODEL_FORWARD_RULES"],
            "new=model"
        );
        assert_eq!(prepared.next["CODEXMANAGER_Q03_UNRELATED"], "keep");
        assert_eq!(
            serde_json::from_str::<BTreeMap<String, String>>(&prepared.raw).unwrap(),
            prepared.next
        );
    }

    #[test]
    fn unrelated_explicit_settings_do_not_expand_sparse_override_map() {
        let previous =
            BTreeMap::from([("CODEXMANAGER_Q03_UNRELATED".to_string(), "keep".to_string())]);
        let mut prepared = PreparedEnvOverrides {
            next: previous.clone(),
            previous: previous.clone(),
            raw: String::new(),
        };
        prepared
            .align_runtime_settings(&BTreeMap::from([
                (
                    APP_SETTING_UPDATE_AUTO_CHECK_KEY.to_string(),
                    "0".to_string(),
                ),
                (
                    APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY.to_string(),
                    "auto".to_string(),
                ),
            ]))
            .expect("align unrelated settings");

        assert_eq!(prepared.next, previous);
    }

    #[test]
    fn effective_environment_matches_reset_and_unchanged_publication() {
        let _guard = crate::test_env_guard();
        let reset_key = "CODEXMANAGER_Q03_EFFECTIVE_RESET";
        let unchanged_key = "CODEXMANAGER_Q03_EFFECTIVE_UNCHANGED";
        let original_reset = std::env::var_os(reset_key);
        let original_unchanged = std::env::var_os(unchanged_key);
        std::env::set_var(reset_key, "startup-baseline");
        super::super::process::apply_env_overrides_to_process(
            &BTreeMap::new(),
            &BTreeMap::from([(reset_key.to_string(), "overridden".to_string())]),
        );
        std::env::set_var(unchanged_key, "newer-explicit-value");
        let prepared = PreparedEnvOverrides {
            previous: BTreeMap::from([
                (reset_key.to_string(), "overridden".to_string()),
                (unchanged_key.to_string(), "stale-snapshot".to_string()),
            ]),
            next: BTreeMap::from([
                (reset_key.to_string(), String::new()),
                (unchanged_key.to_string(), "stale-snapshot".to_string()),
            ]),
            raw: String::new(),
        };

        let effective = prepared.effective_process_env();
        assert_eq!(effective[reset_key], "startup-baseline");
        assert_eq!(effective[unchanged_key], "newer-explicit-value");
        assert_eq!(std::env::var(reset_key).unwrap(), "overridden");
        for (key, original) in [
            (reset_key, original_reset),
            (unchanged_key, original_unchanged),
        ] {
            match original {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}
