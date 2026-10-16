use serde::{Deserialize, Deserializer};
use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;

use super::save_persisted_app_setting;

const SETTING_KEY: &str = "app.client_ip_names";
type ClientIpNames = BTreeMap<String, String>;

fn normalize(names: ClientIpNames) -> Result<ClientIpNames, String> {
    if names.len() > 5000 {
        return Err("clientIpNames must contain at most 5000 entries".to_string());
    }
    let mut normalized = BTreeMap::new();
    for (raw_ip, raw_name) in names {
        let ip = raw_ip
            .parse::<IpAddr>()
            .map_err(|_| "clientIpNames contains an invalid IP address".to_string())?;
        let canonical_ip = match ip {
            IpAddr::V6(ip) => ip
                .to_ipv4_mapped()
                .map(|mapped| mapped.to_string())
                .unwrap_or_else(|| ip.to_string()),
            IpAddr::V4(ip) => ip.to_string(),
        };
        // Reject controls before trimming so leading tabs/newlines cannot slip through.
        if raw_name.chars().any(char::is_control) {
            return Err("clientIpNames names must not contain control characters".to_string());
        }
        let name = raw_name.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err("clientIpNames names must contain 1-100 characters".to_string());
        }
        if let Some(previous) = normalized.insert(canonical_ip, name.to_string()) {
            if previous != name {
                return Err("clientIpNames contains conflicting canonical IP addresses".to_string());
            }
        }
    }
    Ok(normalized)
}

pub(super) fn deserialize_patch<'de, D>(deserializer: D) -> Result<Option<ClientIpNames>, D::Error>
where
    D: Deserializer<'de>,
{
    ensure_local_storage().map_err(serde::de::Error::custom)?;
    // Missing means unchanged; explicit null is not a map and must be rejected.
    let names = ClientIpNames::deserialize(deserializer)?;
    normalize(names).map(Some).map_err(serde::de::Error::custom)
}

pub(super) fn load(settings: &HashMap<String, String>) -> Result<ClientIpNames, String> {
    if crate::storage_helpers::seaorm_enabled() {
        return Ok(BTreeMap::new());
    }
    let Some(raw) = settings.get(SETTING_KEY) else {
        return Ok(BTreeMap::new());
    };
    let names = serde_json::from_str::<ClientIpNames>(raw)
        .map_err(|_| "invalid stored clientIpNames JSON map".to_string())?;
    normalize(names)
}

pub(super) fn save(names: &ClientIpNames) -> Result<(), String> {
    ensure_local_storage()?;
    let raw =
        serde_json::to_string(names).map_err(|_| "serialize clientIpNames failed".to_string())?;
    save_persisted_app_setting(SETTING_KEY, Some(&raw))
}

fn ensure_local_storage() -> Result<(), String> {
    if crate::storage_helpers::seaorm_enabled() {
        return Err("clientIpNames requires the desktop local SQLite store".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_ip_names_remote_mode_rejects_personnel_and_hides_stored_names() {
        let _guard = crate::test_env_guard();
        let previous = std::env::var_os("CODEXMANAGER_STORAGE_BACKEND");
        std::env::set_var("CODEXMANAGER_STORAGE_BACKEND", "postgres");
        let parsed = deserialize_patch(serde_json::json!({"192.0.2.1": "Example Person"}));
        let settings = HashMap::from([(
            SETTING_KEY.to_string(),
            serde_json::json!({"192.0.2.1": "Example Person"}).to_string(),
        )]);
        let loaded = load(&settings);
        match previous {
            Some(value) => std::env::set_var("CODEXMANAGER_STORAGE_BACKEND", value),
            None => std::env::remove_var("CODEXMANAGER_STORAGE_BACKEND"),
        }
        assert!(parsed.is_err(), "remote settings must not accept personnel");
        assert!(loaded.unwrap().is_empty());
    }
}
