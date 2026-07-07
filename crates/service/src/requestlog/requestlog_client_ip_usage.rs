use codexmanager_core::rpc::types::{
    ClientIpUsageListParams, ClientIpUsageListResult, ClientIpUsageSummaryResult,
};
use codexmanager_core::storage::{now_ts, ClientIpUsageSummary, Storage};

use crate::storage_helpers::open_storage;

const DEFAULT_CLIENT_IP_USAGE_LIMIT: i64 = 100;
const MAX_CLIENT_IP_USAGE_LIMIT: i64 = 500;
const DEFAULT_CLIENT_IP_USAGE_RANGE_SECONDS: i64 = 30 * 24 * 60 * 60;

struct NormalizedClientIpUsageParams {
    start_ts: i64,
    end_ts: i64,
    limit: usize,
}

impl NormalizedClientIpUsageParams {
    fn from_params(params: ClientIpUsageListParams) -> Self {
        let now = now_ts();
        let end_ts = params.end_ts.filter(|value| *value > 0).unwrap_or(now + 1);
        let start_ts = params
            .start_ts
            .filter(|value| *value > 0)
            .unwrap_or_else(|| end_ts.saturating_sub(DEFAULT_CLIENT_IP_USAGE_RANGE_SECONDS));
        let (start_ts, end_ts) = if start_ts > end_ts {
            (end_ts, start_ts)
        } else {
            (start_ts, end_ts)
        };
        let limit = params
            .limit
            .unwrap_or(DEFAULT_CLIENT_IP_USAGE_LIMIT)
            .clamp(0, MAX_CLIENT_IP_USAGE_LIMIT) as usize;

        Self {
            start_ts,
            end_ts,
            limit,
        }
    }
}

pub(crate) fn read_client_ip_usage(
    params: ClientIpUsageListParams,
    key_ids: Option<&[String]>,
) -> Result<ClientIpUsageListResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    read_client_ip_usage_with_storage(&storage, params, key_ids)
}

pub(crate) fn read_client_ip_usage_with_storage(
    storage: &Storage,
    params: ClientIpUsageListParams,
    key_ids: Option<&[String]>,
) -> Result<ClientIpUsageListResult, String> {
    let params = NormalizedClientIpUsageParams::from_params(params);
    if params.limit == 0 || key_ids.is_some_and(|ids| ids.is_empty()) {
        return Ok(ClientIpUsageListResult::default());
    }

    let rows = storage
        .summarize_request_token_stats_by_key_and_client_ip_between(
            params.start_ts,
            params.end_ts,
            key_ids,
        )
        .map_err(|err| format!("summarize client ip usage failed: {err}"))?;
    Ok(ClientIpUsageListResult {
        items: rows
            .into_iter()
            .take(params.limit)
            .map(map_client_ip_usage_summary)
            .collect(),
    })
}

fn map_client_ip_usage_summary(item: ClientIpUsageSummary) -> ClientIpUsageSummaryResult {
    ClientIpUsageSummaryResult {
        key_id: item.key_id,
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

#[cfg(test)]
#[path = "requestlog_client_ip_usage_tests.rs"]
mod tests;
