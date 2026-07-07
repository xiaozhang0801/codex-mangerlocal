use codexmanager_core::rpc::types::ClientIpUsageListParams;
use codexmanager_core::storage::{RequestLog, RequestTokenStat, Storage};

use super::read_client_ip_usage_with_storage;

fn insert_usage_row(
    storage: &Storage,
    request_log_id: i64,
    key_id: &str,
    client_ip: &str,
    total_tokens: i64,
    created_at: i64,
) {
    storage
        .insert_request_log(&RequestLog {
            trace_id: Some(format!("trace-client-ip-{request_log_id}")),
            key_id: Some(key_id.to_string()),
            client_ip: Some(client_ip.to_string()),
            request_path: "/v1/responses".to_string(),
            method: "POST".to_string(),
            status_code: Some(200),
            created_at,
            ..RequestLog::default()
        })
        .expect("insert request log");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id,
            key_id: Some(key_id.to_string()),
            client_ip: Some(client_ip.to_string()),
            input_tokens: Some(total_tokens),
            total_tokens: Some(total_tokens),
            estimated_cost_usd: Some(total_tokens as f64 / 1000.0),
            created_at,
            ..RequestTokenStat::default()
        })
        .expect("insert request token stat");
}

#[test]
fn client_ip_usage_reads_admin_rows_sorted_by_tokens() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    insert_usage_row(&storage, 1, "key-a", "192.168.1.23", 30, 1_000);
    insert_usage_row(&storage, 2, "key-a", "192.168.1.24", 80, 1_010);

    let result = read_client_ip_usage_with_storage(
        &storage,
        ClientIpUsageListParams {
            start_ts: Some(900),
            end_ts: Some(2_000),
            limit: Some(10),
        },
        None,
    )
    .expect("read client ip usage");

    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].client_ip, "192.168.1.24");
    assert_eq!(result.items[0].total_tokens, 80);
    assert_eq!(result.items[1].client_ip, "192.168.1.23");
}

#[test]
fn client_ip_usage_filters_to_member_key_ids() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    insert_usage_row(&storage, 11, "member-key", "192.168.1.50", 40, 1_100);
    insert_usage_row(&storage, 12, "other-key", "192.168.1.51", 90, 1_110);

    let result = read_client_ip_usage_with_storage(
        &storage,
        ClientIpUsageListParams {
            start_ts: Some(1_000),
            end_ts: Some(2_000),
            limit: Some(10),
        },
        Some(&["member-key".to_string()]),
    )
    .expect("read member client ip usage");

    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].key_id, "member-key");
    assert_eq!(result.items[0].client_ip, "192.168.1.50");
    assert_eq!(result.items[0].total_tokens, 40);
}
