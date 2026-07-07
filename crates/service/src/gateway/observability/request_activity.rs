use codexmanager_core::rpc::types::{DashboardActiveRequestItem, DashboardActiveRequestsResult};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestActivityStatus {
    Queued,
    Running,
}

impl RequestActivityStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
        }
    }

    fn sort_rank(self) -> i32 {
        match self {
            Self::Running => 0,
            Self::Queued => 1,
        }
    }
}

pub(crate) struct RequestActivityStart<'a> {
    pub(crate) trace_id: &'a str,
    pub(crate) client_ip: Option<&'a str>,
    pub(crate) key_id: &'a str,
    pub(crate) path: &'a str,
    pub(crate) method: &'a str,
    pub(crate) model: Option<&'a str>,
}

#[derive(Debug, Clone)]
struct RequestActivityEntry {
    sequence: u64,
    id: String,
    trace_id: String,
    status: RequestActivityStatus,
    client_ip: Option<String>,
    key_id: String,
    path: String,
    method: String,
    model: Option<String>,
    route_kind: String,
    source_kind: Option<String>,
    source_id: Option<String>,
    created_at_ms: i64,
    queued_at_ms: Option<i64>,
    running_at_ms: Option<i64>,
}

pub(crate) struct RequestActivityGuard {
    trace_id: String,
}

impl Drop for RequestActivityGuard {
    fn drop(&mut self) {
        let Ok(mut entries) = activity_entries().lock() else {
            return;
        };
        entries.remove(self.trace_id.as_str());
    }
}

static ACTIVITY_ENTRIES: OnceLock<Mutex<HashMap<String, RequestActivityEntry>>> = OnceLock::new();
static NEXT_ACTIVITY_ID: AtomicU64 = AtomicU64::new(1);

fn activity_entries() -> &'static Mutex<HashMap<String, RequestActivityEntry>> {
    ACTIVITY_ENTRIES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

pub(crate) fn begin_request_activity(input: RequestActivityStart<'_>) -> RequestActivityGuard {
    let sequence = NEXT_ACTIVITY_ID.fetch_add(1, Ordering::Relaxed);
    let now = now_millis();
    let entry = RequestActivityEntry {
        sequence,
        id: format!("active-{sequence}"),
        trace_id: input.trace_id.to_string(),
        status: RequestActivityStatus::Queued,
        client_ip: input.client_ip.map(str::to_string),
        key_id: input.key_id.to_string(),
        path: input.path.to_string(),
        method: input.method.to_string(),
        model: input.model.map(str::to_string),
        route_kind: "gateway".to_string(),
        source_kind: None,
        source_id: None,
        created_at_ms: now,
        queued_at_ms: Some(now),
        running_at_ms: None,
    };
    if let Ok(mut entries) = activity_entries().lock() {
        entries.insert(input.trace_id.to_string(), entry);
    }
    RequestActivityGuard {
        trace_id: input.trace_id.to_string(),
    }
}

#[cfg(test)]
fn update_request_activity_status(trace_id: &str, status: RequestActivityStatus) {
    update_request_activity_status_with_route(trace_id, status, None);
}

fn update_request_activity_status_with_route(
    trace_id: &str,
    status: RequestActivityStatus,
    route_kind: Option<&str>,
) {
    let Ok(mut entries) = activity_entries().lock() else {
        return;
    };
    let Some(entry) = entries.get_mut(trace_id) else {
        return;
    };
    let now = now_millis();
    entry.status = status;
    if let Some(route_kind) = route_kind.map(str::trim).filter(|value| !value.is_empty()) {
        entry.route_kind = route_kind.to_string();
    }
    match status {
        RequestActivityStatus::Queued => {
            if entry.queued_at_ms.is_none() {
                entry.queued_at_ms = Some(now);
            }
            entry.running_at_ms = None;
        }
        RequestActivityStatus::Running => {
            if entry.running_at_ms.is_none() {
                entry.running_at_ms = Some(now);
            }
        }
    }
}

pub(crate) fn mark_request_activity_queued(trace_id: &str, route_kind: &str) {
    update_request_activity_status_with_route(
        trace_id,
        RequestActivityStatus::Queued,
        Some(route_kind),
    );
}

pub(crate) fn mark_request_activity_running(trace_id: &str, route_kind: &str) {
    update_request_activity_status_with_route(
        trace_id,
        RequestActivityStatus::Running,
        Some(route_kind),
    );
}

pub(crate) fn update_request_activity_source(
    trace_id: &str,
    source_kind: Option<&str>,
    source_id: Option<&str>,
) {
    let Ok(mut entries) = activity_entries().lock() else {
        return;
    };
    let Some(entry) = entries.get_mut(trace_id) else {
        return;
    };
    entry.source_kind = source_kind
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    entry.source_id = source_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
}

pub(crate) fn request_activity_snapshot(limit: usize) -> DashboardActiveRequestsResult {
    let now = now_millis();
    let mut entries = activity_entries()
        .lock()
        .map(|entries| entries.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    entries.sort_by(|left, right| {
        left.status
            .sort_rank()
            .cmp(&right.status.sort_rank())
            .then_with(|| left.created_at_ms.cmp(&right.created_at_ms))
            .then_with(|| left.sequence.cmp(&right.sequence))
    });

    let total_count = entries.len() as i64;
    let queued_count = entries
        .iter()
        .filter(|entry| entry.status == RequestActivityStatus::Queued)
        .count() as i64;
    let running_count = entries
        .iter()
        .filter(|entry| entry.status == RequestActivityStatus::Running)
        .count() as i64;
    let items = entries
        .into_iter()
        .take(limit)
        .map(|entry| DashboardActiveRequestItem {
            id: entry.id,
            trace_id: entry.trace_id,
            status: entry.status.as_str().to_string(),
            client_ip: entry.client_ip,
            key_id: entry.key_id,
            path: entry.path,
            method: entry.method,
            model: entry.model,
            route_kind: entry.route_kind,
            source_kind: entry.source_kind,
            source_id: entry.source_id,
            created_at_ms: entry.created_at_ms,
            queued_at_ms: entry.queued_at_ms,
            running_at_ms: entry.running_at_ms,
            wait_ms: entry
                .running_at_ms
                .or(entry.queued_at_ms)
                .unwrap_or(now)
                .saturating_sub(entry.created_at_ms),
            running_ms: entry
                .running_at_ms
                .map(|running_at_ms| now.saturating_sub(running_at_ms))
                .unwrap_or(0),
        })
        .collect();

    DashboardActiveRequestsResult {
        total_count,
        queued_count,
        running_count,
        items,
    }
}

#[cfg(test)]
pub(crate) fn clear_request_activity_for_tests() {
    if let Ok(mut entries) = activity_entries().lock() {
        entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_guard_removes_request_on_drop() {
        let _guard = crate::test_env_guard();
        clear_request_activity_for_tests();
        let guard = begin_request_activity(RequestActivityStart {
            trace_id: "trc-1",
            client_ip: Some("192.168.1.20"),
            key_id: "key-1",
            path: "/v1/responses",
            method: "POST",
            model: Some("gpt-5"),
        });
        update_request_activity_status("trc-1", RequestActivityStatus::Queued);
        assert_eq!(request_activity_snapshot(50).queued_count, 1);
        drop(guard);
        assert_eq!(request_activity_snapshot(50).total_count, 0);
    }

    #[test]
    fn running_source_and_limit_are_reflected_in_snapshot() {
        let _guard = crate::test_env_guard();
        clear_request_activity_for_tests();
        let guard_a = begin_request_activity(RequestActivityStart {
            trace_id: "trc-a",
            client_ip: Some("192.168.1.20"),
            key_id: "key-a",
            path: "/v1/responses",
            method: "POST",
            model: Some("gpt-5"),
        });
        let _guard_b = begin_request_activity(RequestActivityStart {
            trace_id: "trc-b",
            client_ip: Some("192.168.1.21"),
            key_id: "key-b",
            path: "/v1/chat/completions",
            method: "POST",
            model: None,
        });
        mark_request_activity_running("trc-a", "account_pool");
        update_request_activity_source("trc-a", Some("openai_account"), Some("acc-1"));

        let snapshot = request_activity_snapshot(1);

        assert_eq!(snapshot.total_count, 2);
        assert_eq!(snapshot.running_count, 1);
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].trace_id, "trc-a");
        assert_eq!(snapshot.items[0].status, "running");
        assert_eq!(snapshot.items[0].route_kind, "account_pool");
        assert_eq!(
            snapshot.items[0].source_kind.as_deref(),
            Some("openai_account")
        );
        assert_eq!(snapshot.items[0].source_id.as_deref(), Some("acc-1"));
        drop(guard_a);
    }
}
