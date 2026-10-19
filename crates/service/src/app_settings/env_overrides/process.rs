use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Mutex, OnceLock};

static ENV_OVERRIDE_BASELINE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

/// 函数 `env_override_baseline`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn env_override_baseline() -> &'static Mutex<HashMap<String, Option<String>>> {
    ENV_OVERRIDE_BASELINE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 函数 `env_override_original_process_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn env_override_original_process_value(key: &str) -> Option<String> {
    let baseline =
        crate::lock_utils::lock_recover(env_override_baseline(), "env_override_baseline");
    if let Some(value) = baseline.get(key) {
        return value.clone();
    }
    drop(baseline);
    super::normalize_optional_text(std::env::var(key).ok().as_deref())
}

/// 函数 `apply_env_overrides_to_process`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn apply_env_overrides_to_process(
    previous: &BTreeMap<String, String>,
    next: &BTreeMap<String, String>,
) {
    let mut all_keys = BTreeSet::new();
    all_keys.extend(previous.keys().cloned());
    all_keys.extend(next.keys().cloned());
    if all_keys.is_empty() {
        return;
    }

    let mut baseline =
        crate::lock_utils::lock_recover(env_override_baseline(), "env_override_baseline");
    for key in &all_keys {
        baseline
            .entry(key.clone())
            .or_insert_with(|| super::normalize_optional_text(std::env::var(key).ok().as_deref()));
    }

    for key in all_keys {
        if let Some(value) = next.get(&key) {
            if value.trim().is_empty() {
                if let Some(original) = baseline.get(&key).and_then(|value| value.clone()) {
                    std::env::set_var(&key, original);
                } else {
                    std::env::remove_var(&key);
                }
            } else {
                std::env::set_var(&key, value);
            }
            continue;
        }
        if let Some(original) = baseline.get(&key).and_then(|value| value.clone()) {
            std::env::set_var(&key, original);
        } else {
            std::env::remove_var(&key);
        }
    }
}

/// Publishing a patch must not replay unchanged environment values over a
/// separately edited settings card. Startup uses the full-snapshot function
/// above because persisted overrides may not yet exist in the process.
pub(super) fn publish_env_override_changes(
    previous: &BTreeMap<String, String>,
    next: &BTreeMap<String, String>,
) {
    let changed = previous
        .keys()
        .chain(next.keys())
        .filter(|key| previous.get(*key) != next.get(*key))
        .cloned()
        .collect::<BTreeSet<_>>();
    if changed.is_empty() {
        return;
    }
    let previous_changes = previous
        .iter()
        .filter(|(key, _)| changed.contains(*key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let next_changes = next
        .iter()
        .filter(|(key, _)| changed.contains(*key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();

    // These settings are kept in runtime cells rather than process variables.
    // A broad environment reload must not reset an unrelated card to defaults.
    let account_max_inflight = crate::gateway::account_max_inflight_limit();
    let user_agent_version = crate::gateway::current_codex_user_agent_version();
    let quota_guard = crate::gateway::current_quota_guard_config();
    apply_env_overrides_to_process(&previous_changes, &next_changes);
    if changed.contains("CODEXMANAGER_ROUTE_STRATEGY") {
        crate::gateway::reload_runtime_config_from_env();
    } else {
        crate::gateway::reload_runtime_config_from_env_preserving_route_strategy();
    }
    if !changed.contains("CODEXMANAGER_ACCOUNT_MAX_INFLIGHT") {
        crate::gateway::set_account_max_inflight_limit(account_max_inflight);
    }
    // No environment variable controls this version: reload_from_env always
    // resets it to the built-in default.
    let _ = crate::gateway::set_codex_user_agent_version(&user_agent_version);

    let mut restored_quota_guard = crate::gateway::current_quota_guard_config();
    if !changed.contains("CODEXMANAGER_QUOTA_GUARD_ENABLED") {
        restored_quota_guard.enabled = quota_guard.enabled;
    }
    if !changed.contains("CODEXMANAGER_QUOTA_GUARD_5H_MIN_REMAINING_PERCENT")
        && !changed.contains("CODEXMANAGER_LOW_QUOTA_THRESHOLD_PERCENT")
    {
        restored_quota_guard.primary_min_remaining_percent =
            quota_guard.primary_min_remaining_percent;
    }
    if !changed.contains("CODEXMANAGER_QUOTA_GUARD_WEEKLY_MIN_REMAINING_PERCENT")
        && !changed.contains("CODEXMANAGER_LOW_QUOTA_THRESHOLD_PERCENT")
    {
        restored_quota_guard.secondary_min_remaining_percent =
            quota_guard.secondary_min_remaining_percent;
    }
    if !changed.contains("CODEXMANAGER_QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK") {
        restored_quota_guard.allow_all_low_quota_fallback =
            quota_guard.allow_all_low_quota_fallback;
    }
    crate::gateway::set_quota_guard_config(restored_quota_guard);
    // All BackgroundTasksSettings fields are reserved for their settings card,
    // so an environment-editor patch cannot change any of those fields.
    crate::usage_http::reload_usage_http_client_from_env();
}

/// 函数 `reload_runtime_after_env_override_apply`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn reload_runtime_after_env_override_apply() {
    if std::env::var("CODEXMANAGER_ROUTE_STRATEGY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
    {
        crate::gateway::reload_runtime_config_from_env();
    } else {
        // runtime_sync applies any persisted strategy after this reload. Keep
        // the current mode until then, so an unchanged persisted value cannot
        // lose its scheduling cursor through a transient default mode.
        crate::gateway::reload_runtime_config_from_env_preserving_route_strategy();
    }
    crate::usage_refresh::reload_background_tasks_runtime_from_env();
    crate::usage_http::reload_usage_http_client_from_env();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_override_does_not_overwrite_a_newer_process_value() {
        let _guard = crate::test_env_guard();
        let key = "CODEXMANAGER_Q03_UNCHANGED_PROCESS";
        let original = std::env::var_os(key);
        std::env::set_var(key, "newer-explicit-setting");
        let unchanged = BTreeMap::from([(key.to_string(), "stale-snapshot".to_string())]);
        publish_env_override_changes(&unchanged, &unchanged);
        assert_eq!(std::env::var(key).unwrap(), "newer-explicit-setting");
        match original {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn unrelated_override_preserves_runtime_only_settings() {
        let _guard = crate::test_env_guard();
        let key = "CODEXMANAGER_Q03_RUNTIME_PRESERVATION";
        let original_env = std::env::var_os(key);
        let original_route = crate::gateway::current_route_strategy();
        let original_inflight = crate::gateway::account_max_inflight_limit();
        let original_version = crate::gateway::current_codex_user_agent_version();
        let original_quota = crate::gateway::current_quota_guard_config();
        let target_route = if original_route == "ordered" {
            "balanced"
        } else {
            "ordered"
        };
        crate::gateway::set_route_strategy(target_route).unwrap();
        crate::gateway::set_account_max_inflight_limit(17);
        crate::gateway::set_codex_user_agent_version("9.9.9").unwrap();
        let target_quota = crate::gateway::QuotaGuardConfig {
            enabled: false,
            primary_min_remaining_percent: 19.0,
            secondary_min_remaining_percent: 29.0,
            allow_all_low_quota_fallback: false,
        };
        crate::gateway::set_quota_guard_config(target_quota);

        publish_env_override_changes(
            &BTreeMap::new(),
            &BTreeMap::from([(key.to_string(), "changed".to_string())]),
        );

        assert_eq!(crate::gateway::current_route_strategy(), target_route);
        assert_eq!(crate::gateway::account_max_inflight_limit(), 17);
        assert_eq!(crate::gateway::current_codex_user_agent_version(), "9.9.9");
        assert_eq!(crate::gateway::current_quota_guard_config(), target_quota);
        crate::gateway::set_route_strategy(original_route).unwrap();
        crate::gateway::set_account_max_inflight_limit(original_inflight);
        crate::gateway::set_codex_user_agent_version(&original_version).unwrap();
        crate::gateway::set_quota_guard_config(original_quota);
        match original_env {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
