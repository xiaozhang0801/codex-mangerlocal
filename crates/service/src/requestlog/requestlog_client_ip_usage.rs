use codexmanager_core::rpc::types::{
    ClientIpUsageListParams, ClientIpUsageListResult, ClientIpUsageSummaryResult,
};
use codexmanager_core::storage::{now_ts, ClientIpUsageSummary, Storage};

use crate::storage_helpers::open_storage;

const DEFAULT_CLIENT_IP_USAGE_LOOKBACK_SECS: i64 = 30 * 24 * 60 * 60;
const DEFAULT_CLIENT_IP_USAGE_LIMIT: i64 = 100;
const MAX_CLIENT_IP_USAGE_LIMIT: i64 = 500;

fn normalize_limit(limit: Option<i64>) -> usize {
    limit
        .unwrap_or(DEFAULT_CLIENT_IP_USAGE_LIMIT)
        .clamp(1, MAX_CLIENT_IP_USAGE_LIMIT) as usize
}

fn normalize_usage_bounds(params: &ClientIpUsageListParams) -> (i64, i64) {
    let (start_ts, end_ts) = super::list::normalize_time_range(params.start_ts, params.end_ts);
    let end_ts = end_ts.unwrap_or_else(now_ts);
    let start_ts =
        start_ts.unwrap_or_else(|| end_ts.saturating_sub(DEFAULT_CLIENT_IP_USAGE_LOOKBACK_SECS));
    (start_ts, end_ts)
}

fn to_client_ip_usage_summary_result(item: ClientIpUsageSummary) -> ClientIpUsageSummaryResult {
    ClientIpUsageSummaryResult {
        client_ip: item.client_ip,
        request_count: item.usage.request_count.max(0),
        success_count: item.usage.success_count.max(0),
        error_count: item.usage.error_count.max(0),
        input_tokens: item.usage.input_tokens.max(0),
        cached_input_tokens: item.usage.cached_input_tokens.max(0),
        output_tokens: item.usage.output_tokens.max(0),
        reasoning_output_tokens: item.usage.reasoning_output_tokens.max(0),
        total_tokens: item.usage.total_tokens.max(0),
        estimated_cost_usd: item.usage.estimated_cost_usd.max(0.0),
        last_seen_at: item.last_seen_at.max(0),
    }
}

pub(crate) fn read_client_ip_usage(
    params: ClientIpUsageListParams,
) -> Result<ClientIpUsageListResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    read_client_ip_usage_with_storage(&storage, params, None)
}

pub(crate) fn read_client_ip_usage_with_storage(
    storage: &Storage,
    params: ClientIpUsageListParams,
    key_ids: Option<&[String]>,
) -> Result<ClientIpUsageListResult, String> {
    if key_ids.is_some_and(|key_ids| key_ids.is_empty()) {
        return Ok(ClientIpUsageListResult::default());
    }
    let (start_ts, end_ts) = normalize_usage_bounds(&params);
    if end_ts <= start_ts {
        return Ok(ClientIpUsageListResult::default());
    }
    let limit = normalize_limit(params.limit);
    let mut items = storage
        .summarize_request_token_stats_by_client_ip_between(start_ts, end_ts, key_ids)
        .map_err(|err| format!("summarize client ip usage failed: {err}"))?
        .into_iter()
        .map(to_client_ip_usage_summary_result)
        .collect::<Vec<_>>();
    items.truncate(limit);
    Ok(ClientIpUsageListResult { items })
}

#[cfg(test)]
mod tests {
    use super::{read_client_ip_usage_with_storage, ClientIpUsageListParams};
    use codexmanager_core::storage::{RequestTokenStat, Storage};

    fn insert_stat(
        storage: &Storage,
        request_log_id: i64,
        key_id: &str,
        client_ip: Option<&str>,
        status_total_tokens: i64,
        created_at: i64,
    ) {
        storage
            .insert_request_token_stat(&RequestTokenStat {
                request_log_id,
                key_id: Some(key_id.to_string()),
                client_ip: client_ip.map(str::to_string),
                model: Some("gpt-5-mini".to_string()),
                input_tokens: Some(status_total_tokens),
                cached_input_tokens: Some(0),
                output_tokens: Some(0),
                total_tokens: Some(status_total_tokens),
                estimated_cost_usd: Some(0.01),
                created_at,
                ..RequestTokenStat::default()
            })
            .expect("insert token stat");
    }

    #[test]
    fn client_ip_usage_merges_same_ip_across_keys_without_key_id() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        insert_stat(&storage, 1, "key-a", Some("192.168.1.20"), 100, 10);
        insert_stat(&storage, 2, "key-b", Some("192.168.1.20"), 200, 20);
        insert_stat(&storage, 3, "key-c", Some("192.168.1.21"), 50, 30);

        let result = read_client_ip_usage_with_storage(
            &storage,
            ClientIpUsageListParams {
                start_ts: Some(0),
                end_ts: Some(100),
                limit: Some(10),
            },
            None,
        )
        .expect("read client ip usage");

        assert_eq!(result.items.len(), 2);
        assert_eq!(result.items[0].client_ip, "192.168.1.20");
        assert_eq!(result.items[0].total_tokens, 300);
        assert_eq!(result.items[0].request_count, 2);
    }

    #[test]
    fn client_ip_usage_filters_key_scope_but_still_groups_by_ip() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        insert_stat(&storage, 1, "key-a", Some("192.168.1.20"), 100, 10);
        insert_stat(&storage, 2, "key-b", Some("192.168.1.20"), 200, 20);

        let selected_keys = vec!["key-a".to_string()];
        let result = read_client_ip_usage_with_storage(
            &storage,
            ClientIpUsageListParams {
                start_ts: Some(0),
                end_ts: Some(100),
                limit: Some(10),
            },
            Some(&selected_keys),
        )
        .expect("read scoped client ip usage");

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].client_ip, "192.168.1.20");
        assert_eq!(result.items[0].total_tokens, 100);
    }
}
