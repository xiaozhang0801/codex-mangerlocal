use super::super::{env_overrides, service, shared::*, ui};
use super::author_links::{
    normalize_author_link_items, serialize_author_link_items, AuthorLinkItem,
};
use super::{BackgroundTasksInput, QuotaGuardInput};
use crate::app_settings::client_ip_names;
use crate::{gateway, usage_refresh};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AppSettingsPatch {
    #[serde(default, deserialize_with = "client_ip_names::deserialize_patch")]
    client_ip_names: Option<BTreeMap<String, String>>,
    account_max_concurrent: Option<usize>,
    client_ip_single_max_concurrent: Option<usize>,
    client_ip_multi_max_concurrent: Option<usize>,
    update_auto_check: Option<bool>,
    remove_requires_openai_auth: Option<bool>,
    auto_start_enabled: Option<bool>,
    show_main_window_on_startup: Option<bool>,
    close_to_tray_on_close: Option<bool>,
    keep_window_ui_mounted: Option<bool>,
    lightweight_mode_on_close_to_tray: Option<bool>,
    codex_cli_guide_dismissed: Option<bool>,
    low_transparency: Option<bool>,
    zoom_factor: Option<f64>,
    theme: Option<String>,
    appearance_preset: Option<String>,
    locale: Option<String>,
    service_addr: Option<String>,
    pub(super) service_listen_mode: Option<String>,
    route_strategy: Option<String>,
    free_account_max_model: Option<String>,
    model_forward_rules: Option<String>,
    compact_model_forward_rules: Option<String>,
    account_max_inflight: Option<usize>,
    thread_aware_account_distribution_enabled: Option<bool>,
    request_log_payload_redaction_enabled: Option<bool>,
    request_log_payload_preview_enabled: Option<bool>,
    request_compression_enabled: Option<bool>,
    gateway_originator: Option<String>,
    gateway_user_agent: Option<String>,
    gateway_user_agent_version: Option<String>,
    gateway_residency_requirement: Option<String>,
    plugin_market_mode: Option<String>,
    plugin_market_source_url: Option<String>,
    author_sponsors: Option<Vec<AuthorLinkItem>>,
    author_server_recommendations: Option<Vec<AuthorLinkItem>>,
    upstream_proxy_url: Option<String>,
    upstream_proxy_bypass_hosts: Option<String>,
    upstream_stream_timeout_ms: Option<u64>,
    upstream_total_timeout_ms: Option<u64>,
    sse_keepalive_enabled: Option<bool>,
    sse_keepalive_interval_ms: Option<u64>,
    quota_guard: Option<QuotaGuardInput>,
    background_tasks: Option<BackgroundTasksInput>,
    env_overrides: Option<HashMap<String, String>>,
    web_access_password: Option<String>,
    web_auth_mode: Option<String>,
    distribution_enabled: Option<bool>,
}

/// 函数 `parse_app_settings_patch`
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
pub(super) fn parse_app_settings_patch(params: Option<&Value>) -> Result<AppSettingsPatch, String> {
    match params {
        Some(value) => serde_json::from_value::<AppSettingsPatch>(value.clone())
            .map_err(|err| format!("invalid app settings payload: {err}")),
        None => Ok(AppSettingsPatch::default()),
    }
}

/// Preparation has no writes. The commit contains only this patch's keys.
pub(super) struct PreparedAppSettingsPatch {
    pub settings: BTreeMap<String, String>,
    runtime: Vec<Box<dyn FnOnce() -> Result<(), String>>>,
}

impl PreparedAppSettingsPatch {
    fn save(&mut self, key: &str, value: impl ToString) {
        self.settings.insert(key.to_owned(), value.to_string());
    }

    pub fn publish(self) -> Result<(), String> {
        for publish in self.runtime {
            publish().map_err(|err| format!(
                "settings_persisted_runtime_apply_failed: settings were saved; restart the service to reapply them: {err}"
            ))?;
        }
        Ok(())
    }
}

pub(super) fn prepare_app_settings_patch(
    patch: AppSettingsPatch,
) -> Result<PreparedAppSettingsPatch, String> {
    let mut plan = PreparedAppSettingsPatch {
        settings: BTreeMap::new(),
        runtime: Vec::new(),
    };
    macro_rules! runtime {
        ($key:expr, $value:expr, $publish:expr) => {{
            plan.save($key, $value);
            plan.runtime.push(Box::new($publish));
        }};
    }
    if let Some(names) = patch.client_ip_names {
        plan.save(
            client_ip_names::SETTING_KEY,
            client_ip_names::prepare(&names)?,
        );
    }
    if let Some(value) = patch.account_max_concurrent {
        let value = value.max(1);
        runtime!(
            APP_SETTING_GATEWAY_ACCOUNT_MAX_CONCURRENT_KEY,
            value,
            move || {
                gateway::set_account_max_concurrent_limit(value);
                Ok(())
            }
        );
    }
    if let Some(value) = patch.client_ip_single_max_concurrent {
        let value = value.max(1);
        runtime!(
            APP_SETTING_GATEWAY_CLIENT_IP_SINGLE_MAX_CONCURRENT_KEY,
            value,
            move || {
                gateway::set_client_ip_single_max_concurrent_limit(value);
                Ok(())
            }
        );
    }
    if let Some(value) = patch.client_ip_multi_max_concurrent {
        let value = value.max(1);
        runtime!(
            APP_SETTING_GATEWAY_CLIENT_IP_MULTI_MAX_CONCURRENT_KEY,
            value,
            move || {
                gateway::set_client_ip_multi_max_concurrent_limit(value);
                Ok(())
            }
        );
    }
    // Legacy compatibility only; never change Gateway authentication.
    let _ = patch.remove_requires_openai_auth;
    if let Some(value) = patch.update_auto_check {
        plan.save(
            APP_SETTING_UPDATE_AUTO_CHECK_KEY,
            if value { "1" } else { "0" },
        );
    }
    if let Some(value) = patch.auto_start_enabled {
        plan.save(
            APP_SETTING_AUTO_START_ENABLED_KEY,
            if value { "1" } else { "0" },
        );
    }
    if let Some(value) = patch.show_main_window_on_startup {
        plan.save(
            APP_SETTING_SHOW_MAIN_WINDOW_ON_STARTUP_KEY,
            if value { "1" } else { "0" },
        );
    }
    if let Some(value) = patch.close_to_tray_on_close {
        plan.save(
            APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY,
            if value { "1" } else { "0" },
        );
    }
    if let Some(value) = patch.codex_cli_guide_dismissed {
        plan.save(
            APP_SETTING_UI_CODEX_CLI_GUIDE_DISMISSED_KEY,
            if value { "1" } else { "0" },
        );
    }
    if let Some(value) = patch.low_transparency {
        plan.save(
            APP_SETTING_UI_LOW_TRANSPARENCY_KEY,
            if value { "1" } else { "0" },
        );
    }
    // The modern field retains precedence over the legacy inverse field.
    if let Some(keep) = patch
        .keep_window_ui_mounted
        .or(patch.lightweight_mode_on_close_to_tray.map(|v| !v))
    {
        plan.save(
            APP_SETTING_KEEP_WINDOW_UI_MOUNTED_KEY,
            if keep { "1" } else { "0" },
        );
        plan.save(
            APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY,
            if keep { "0" } else { "1" },
        );
    }
    if let Some(value) = patch.zoom_factor {
        plan.save(
            APP_SETTING_UI_ZOOM_FACTOR_KEY,
            ui::normalize_ui_zoom_factor(value),
        );
    }
    if let Some(value) = patch.theme {
        plan.save(
            APP_SETTING_UI_THEME_KEY,
            ui::normalize_ui_theme(Some(&value)),
        );
    }
    if let Some(value) = patch.appearance_preset {
        plan.save(
            APP_SETTING_UI_APPEARANCE_PRESET_KEY,
            ui::normalize_ui_appearance_preset(Some(&value)),
        );
    }
    if let Some(value) = patch.locale {
        plan.save(
            APP_SETTING_UI_LOCALE_KEY,
            ui::normalize_ui_locale(Some(&value)),
        );
    }
    let next_addr = patch
        .service_addr
        .as_deref()
        .map(|v| service::normalize_saved_service_addr(Some(v)))
        .transpose()?;
    if let Some(addr) = &next_addr {
        plan.save(APP_SETTING_SERVICE_ADDR_KEY, addr);
    }
    if let Some(value) = patch.service_listen_mode {
        let mode = service::normalize_service_bind_mode(Some(&value));
        let current_addr = next_addr.unwrap_or_else(service::current_saved_service_addr);
        let addr = service::listener_bind_addr_for_mode(&current_addr, mode);
        plan.save(service::SERVICE_BIND_MODE_SETTING_KEY, mode);
        runtime!(APP_SETTING_SERVICE_ADDR_KEY, &addr, move || {
            std::env::set_var("CODEXMANAGER_SERVICE_ADDR", addr);
            Ok(())
        });
    }
    if let Some(value) = patch.route_strategy {
        let value = gateway::normalize_route_strategy(&value)?.to_string();
        runtime!(APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY, &value, move || {
            gateway::set_route_strategy(&value).map(|_| ())
        });
    }
    if let Some(value) = patch.free_account_max_model {
        let value = gateway::normalize_free_account_max_model(&value)?.to_string();
        runtime!(
            APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY,
            &value,
            move || gateway::set_free_account_max_model(&value).map(|_| ())
        );
    }
    if let Some(value) = patch.model_forward_rules {
        let value = gateway::normalize_model_forward_rules(&value)?.to_string();
        runtime!(
            APP_SETTING_GATEWAY_MODEL_FORWARD_RULES_KEY,
            &value,
            move || gateway::set_model_forward_rules(&value).map(|_| ())
        );
    }
    if let Some(value) = patch.compact_model_forward_rules {
        let value = gateway::normalize_model_forward_rules(&value)?.to_string();
        runtime!(
            APP_SETTING_GATEWAY_COMPACT_MODEL_FORWARD_RULES_KEY,
            &value,
            move || gateway::set_compact_model_forward_rules(&value).map(|_| ())
        );
    }
    if let Some(value) = patch.gateway_originator {
        let value = gateway::normalize_originator(&value)?.to_string();
        runtime!(APP_SETTING_GATEWAY_ORIGINATOR_KEY, &value, move || {
            gateway::set_originator(&value).map(|_| ())
        });
    }
    if let Some(value) = patch.gateway_user_agent_version {
        let value = gateway::normalize_codex_user_agent_version(&value)?.to_string();
        runtime!(
            APP_SETTING_GATEWAY_USER_AGENT_VERSION_KEY,
            &value,
            move || gateway::set_codex_user_agent_version(&value).map(|_| ())
        );
    }
    if let Some(value) = patch.account_max_inflight {
        runtime!(
            APP_SETTING_GATEWAY_ACCOUNT_MAX_INFLIGHT_KEY,
            value,
            move || {
                gateway::set_account_max_inflight_limit(value);
                Ok(())
            }
        );
    }
    if let Some(value) = patch.upstream_stream_timeout_ms {
        runtime!(
            APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
            value,
            move || {
                gateway::set_upstream_stream_timeout_ms(value);
                Ok(())
            }
        );
    }
    if let Some(value) = patch.upstream_total_timeout_ms {
        runtime!(
            APP_SETTING_GATEWAY_UPSTREAM_TOTAL_TIMEOUT_MS_KEY,
            value,
            move || {
                gateway::set_upstream_total_timeout_ms(value);
                Ok(())
            }
        );
    }
    if let Some(value) = patch.thread_aware_account_distribution_enabled {
        runtime!(
            APP_SETTING_GATEWAY_THREAD_AWARE_ACCOUNT_DISTRIBUTION_ENABLED_KEY,
            if value { "1" } else { "0" },
            move || {
                gateway::set_thread_aware_account_distribution_enabled(value);
                Ok(())
            }
        );
    }
    if let Some(value) = patch.request_log_payload_redaction_enabled {
        runtime!(
            APP_SETTING_GATEWAY_REQUEST_LOG_PAYLOAD_REDACTION_ENABLED_KEY,
            if value { "1" } else { "0" },
            move || {
                gateway::set_request_log_payload_redaction_enabled(value);
                Ok(())
            }
        );
    }
    if let Some(value) = patch.request_log_payload_preview_enabled {
        runtime!(
            APP_SETTING_GATEWAY_REQUEST_LOG_PAYLOAD_PREVIEW_ENABLED_KEY,
            if value { "1" } else { "0" },
            move || {
                gateway::set_request_log_payload_preview_enabled(value);
                Ok(())
            }
        );
    }
    if let Some(value) = patch.sse_keepalive_enabled {
        runtime!(
            APP_SETTING_GATEWAY_SSE_KEEPALIVE_ENABLED_KEY,
            if value { "1" } else { "0" },
            move || {
                gateway::set_sse_keepalive_enabled(value);
                Ok(())
            }
        );
    }
    if let Some(value) = patch.request_compression_enabled {
        runtime!(
            APP_SETTING_GATEWAY_REQUEST_COMPRESSION_ENABLED_KEY,
            if value { "1" } else { "0" },
            move || {
                gateway::set_request_compression_enabled(value);
                Ok(())
            }
        );
    }
    if let Some(value) = patch.gateway_user_agent {
        let value = gateway::normalize_gateway_user_agent(Some(&value))?;
        runtime!(
            APP_SETTING_GATEWAY_USER_AGENT_KEY,
            value.as_deref().unwrap_or_default(),
            move || gateway::set_gateway_user_agent(value.as_deref()).map(|_| ())
        );
    }
    if let Some(value) = patch.gateway_residency_requirement {
        let value = gateway::normalize_residency_requirement(Some(&value))?;
        runtime!(
            APP_SETTING_GATEWAY_RESIDENCY_REQUIREMENT_KEY,
            value.as_deref().unwrap_or_default(),
            move || gateway::set_residency_requirement(value.as_deref()).map(|_| ())
        );
    }
    if let Some(value) = patch.plugin_market_mode {
        plan.save(APP_SETTING_PLUGIN_MARKET_MODE_KEY, value.trim());
    }
    if let Some(value) = patch.plugin_market_source_url {
        plan.save(APP_SETTING_PLUGIN_MARKET_SOURCE_URL_KEY, value.trim());
    }
    if let Some(value) = patch.author_sponsors {
        plan.save(
            APP_SETTING_AUTHOR_SPONSORS_KEY,
            serialize_author_link_items(&normalize_author_link_items(value))?,
        );
    }
    if let Some(value) = patch.author_server_recommendations {
        plan.save(
            APP_SETTING_AUTHOR_SERVER_RECOMMENDATIONS_KEY,
            serialize_author_link_items(&normalize_author_link_items(value))?,
        );
    }
    if let Some(value) = patch.upstream_proxy_bypass_hosts {
        if value.contains(char::from(0)) {
            return Err("upstream proxy bypass hosts contains NUL".into());
        }
        let value = gateway::normalize_upstream_proxy_bypass_hosts(Some(&value));
        runtime!(
            APP_SETTING_GATEWAY_UPSTREAM_PROXY_BYPASS_HOSTS_KEY,
            &value,
            move || {
                gateway::set_upstream_proxy_bypass_hosts(Some(&value));
                Ok(())
            }
        );
    }
    if let Some(value) = patch.sse_keepalive_interval_ms {
        let value = gateway::normalize_sse_keepalive_interval_ms(value)?;
        runtime!(
            APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY,
            value,
            move || gateway::set_sse_keepalive_interval_ms(value).map(|_| ())
        );
    }
    if let Some(input) = patch.quota_guard {
        let value = input.into_config().normalized();
        runtime!(
            APP_SETTING_GATEWAY_QUOTA_GUARD_KEY,
            serde_json::to_string(&value).map_err(|e| e.to_string())?,
            move || {
                gateway::set_quota_guard_config(value);
                Ok(())
            }
        );
    }
    if let Some(input) = patch.background_tasks {
        let value = usage_refresh::prepare_background_tasks_settings(input.into_patch())?;
        runtime!(
            APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY,
            serde_json::to_string(&value.snapshot).map_err(|e| e.to_string())?,
            move || {
                usage_refresh::publish_background_tasks_settings(value);
                Ok(())
            }
        );
    }
    let mut environment = if let Some(input) = patch.env_overrides {
        Some(env_overrides::prepare_env_overrides(input)?)
    } else {
        env_overrides::prepare_env_overrides_for_settings(&plan.settings)?
    };
    if let Some(prepared) = &mut environment {
        prepared.align_runtime_settings(&plan.settings)?;
    }
    if let Some(value) = patch.upstream_proxy_url {
        if value.contains(char::from(0)) {
            return Err("upstream proxy URL contains NUL".into());
        }
        let prepared = if let Some(environment) = &environment {
            gateway::prepare_upstream_proxy_url_with_env(
                Some(&value),
                &environment.effective_process_env(),
            )?
        } else {
            gateway::prepare_upstream_proxy_url(Some(&value))?
        };
        runtime!(
            APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY,
            prepared.normalized_url().unwrap_or_default(),
            move || {
                prepared.publish();
                Ok(())
            }
        );
    }
    if let Some(prepared) = environment {
        plan.save(APP_SETTING_ENV_OVERRIDES_KEY, &prepared.raw);
        // Publish the environment first, then the explicit typed fields. The
        // aligned persisted override snapshot uses that same precedence.
        plan.runtime.insert(
            0,
            Box::new(move || {
                prepared.publish();
                Ok(())
            }),
        );
    }
    crate::auth::app_manager::prepare_web_settings_patch(
        patch.web_access_password.as_deref(),
        patch.web_auth_mode.as_deref(),
        patch.distribution_enabled,
        &mut plan.settings,
    )?;
    Ok(plan)
}
