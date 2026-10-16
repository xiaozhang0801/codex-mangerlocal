use super::with_temp_db;
use codexmanager_core::storage::Storage;
use codexmanager_service::{app_settings_get, app_settings_get_with_overrides, app_settings_set};
use serde_json::{json, Map, Value};
use std::path::Path;

fn stored_names(path: &Path) -> Value {
    let storage = Storage::open(path).expect("reopen storage");
    let raw = storage
        .get_app_setting("app.client_ip_names")
        .expect("read bindings")
        .expect("bindings persisted");
    serde_json::from_str(&raw).expect("stored bindings JSON")
}

#[test]
fn defaults_to_empty_map() {
    with_temp_db(|_| {
        assert_eq!(app_settings_get().unwrap()["clientIpNames"], json!({}));
        assert_eq!(
            app_settings_get_with_overrides(Some(true), Some(true)).unwrap()["clientIpNames"],
            json!({})
        );
    });
}

#[test]
fn roundtrips_trimmed_names_through_response_and_reopened_storage() {
    with_temp_db(|path| {
        let expected = json!({"192.0.2.10": "Example Person", "2001:db8::10": "Example Two"});
        let response = app_settings_set(Some(&json!({"clientIpNames": {
            "192.0.2.10": "  Example Person  ", "2001:db8::10": "Example Two"
        }})))
        .unwrap();
        assert_eq!(response["clientIpNames"], expected);
        assert_eq!(stored_names(path), expected);
        assert_eq!(stored_names(path), expected);
        assert_eq!(app_settings_get().unwrap()["clientIpNames"], expected);
        assert_eq!(
            app_settings_set(Some(&response)).unwrap()["clientIpNames"],
            expected
        );
    });
}

#[test]
fn replaces_clears_and_preserves_when_omitted() {
    with_temp_db(|path| {
        app_settings_set(Some(
            &json!({"clientIpNames": {"192.0.2.1": "Example One"}}),
        ))
        .unwrap();
        let replacement = json!({"192.0.2.2": "Example Two"});
        let response = app_settings_set(Some(&json!({"clientIpNames": replacement}))).unwrap();
        assert_eq!(response["clientIpNames"], replacement);
        assert_eq!(stored_names(path), replacement);
        assert_eq!(
            app_settings_set(None).unwrap()["clientIpNames"],
            replacement
        );
        let cleared = app_settings_set(Some(&json!({"clientIpNames": {}}))).unwrap();
        assert_eq!(cleared["clientIpNames"], json!({}));
        assert_eq!(stored_names(path), json!({}));
        assert_eq!(app_settings_get().unwrap()["clientIpNames"], json!({}));
    });
}

#[test]
fn canonicalizes_ipv6_and_mapped_ipv4_and_merges_equal_names() {
    with_temp_db(|path| {
        let response = app_settings_set(Some(&json!({"clientIpNames": {
            "2001:0DB8:0000:0000:0000:0000:0000:000A": "Example V6",
            "2001:db8::a": " Example V6 ",
            "::ffff:192.0.2.10": "Example V4",
            "::FFFF:C000:020A": " Example V4 ",
            "192.0.2.10": "Example V4"
        }})))
        .unwrap();
        let expected = json!({"2001:db8::a": "Example V6", "192.0.2.10": "Example V4"});
        assert_eq!(response["clientIpNames"], expected);
        assert_eq!(stored_names(path), expected);
    });
}

#[test]
fn rejects_invalid_maps_before_any_patch_writes() {
    with_temp_db(|path| {
        let original = json!({"192.0.2.1": "Original Example"});
        let before = app_settings_set(Some(&json!({"clientIpNames": original}))).unwrap();
        let invalid_maps = vec![
            json!(null),
            json!([]),
            json!("invalid"),
            json!(true),
            json!(42),
            json!({"not-an-ip": "Example"}),
            json!({"192.0.2.999": "Example"}),
            json!({"192.0.2.1:80": "Example"}),
            json!({"192.0.2.0/24": "Example"}),
            json!({"[2001:db8::1]": "Example"}),
            json!({"2001:db8::1%eth0": "Example"}),
            json!({"192.0.2.1": ""}),
            json!({"192.0.2.1": "   "}),
            json!({"192.0.2.1": "Example\nName"}),
            json!({"192.0.2.1": "\tExample"}),
            json!({"192.0.2.1": "Example\u{007f}"}),
            json!({"192.0.2.1": "Example\u{0085}"}),
            json!({"192.0.2.1": "x".repeat(101)}),
            json!({"192.0.2.1": null}),
            json!({"192.0.2.1": 12}),
            json!({"192.0.2.1": "Example One", "::ffff:192.0.2.1": "Example Two"}),
            json!({"2001:db8::1": "Example One", "2001:0DB8:0:0:0:0:0:1": "Example Two"}),
            json!({"192.0.2.2": "Valid Example", "2001:db8::2": ""}),
        ];
        for (index, invalid) in invalid_maps.into_iter().enumerate() {
            let result = app_settings_set(Some(&json!({
                "updateAutoCheck": !before["updateAutoCheck"].as_bool().unwrap(),
                "clientIpNames": invalid
            })));
            assert!(result.is_err(), "invalid map case {index} accepted");
            assert_eq!(stored_names(path), original, "case {index}");
            let after = app_settings_get().unwrap();
            assert_eq!(after["clientIpNames"], original, "case {index}");
            assert_eq!(
                after["updateAutoCheck"], before["updateAutoCheck"],
                "case {index}"
            );
        }
    });
}

#[test]
fn enforces_entry_limit_and_counts_unicode_name_characters() {
    with_temp_db(|path| {
        let name = "\u{4f8b}".repeat(100);
        let mut names = (1..=5000)
            .map(|index| (format!("2001:db8::{index:x}"), json!(name)))
            .collect::<Map<String, Value>>();
        let response = app_settings_set(Some(&json!({"clientIpNames": names}))).unwrap();
        assert_eq!(response["clientIpNames"].as_object().unwrap().len(), 5000);
        assert_eq!(response["clientIpNames"]["2001:db8::1"], name);
        names.insert("192.0.2.1".into(), json!("Extra Example"));
        assert!(app_settings_set(Some(&json!({"clientIpNames": names}))).is_err());
        assert_eq!(stored_names(path), response["clientIpNames"]);
        assert!(app_settings_set(Some(&json!({"clientIpNames": {
            "192.0.2.1": "\u{4f8b}".repeat(101)
        }})))
        .is_err());
        assert_eq!(stored_names(path), response["clientIpNames"]);
    });
}

#[test]
fn binding_changes_leave_concurrency_settings_unchanged() {
    with_temp_db(|path| {
        let before = app_settings_set(Some(&json!({
            "accountMaxConcurrent": 7,
            "clientIpSingleMaxConcurrent": 4,
            "clientIpMultiMaxConcurrent": 3
        })))
        .unwrap();
        let persisted_before = Storage::open(path).unwrap().list_app_settings().unwrap();
        for names in [json!({"192.0.2.1": "Example Person"}), json!({})] {
            let response = app_settings_set(Some(&json!({"clientIpNames": names}))).unwrap();
            assert_eq!(response["clientIpNames"], names);
            for key in [
                "accountMaxInflight",
                "accountMaxConcurrent",
                "clientIpSingleMaxConcurrent",
                "clientIpMultiMaxConcurrent",
            ] {
                assert_eq!(response[key], before[key], "{key}");
            }
            let persisted_after = Storage::open(path).unwrap().list_app_settings().unwrap();
            for (key, value) in &persisted_before {
                if key.contains("concurrent") || key.contains("inflight") {
                    assert!(
                        persisted_after.contains(&(key.clone(), value.clone())),
                        "{key}"
                    );
                }
            }
        }
    });
}
