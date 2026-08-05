use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use codexmanager_core::rpc::types::{
    DashboardActiveRequestIpGroup, DashboardActiveRequestItem, DashboardActiveRequestsResult,
};

const STATUS_QUEUED: &str = "queued";
const STATUS_RUNNING: &str = "running";

static REQUEST_ACTIVITY: OnceLock<Mutex<HashMap<String, RequestActivityEntry>>> = OnceLock::new();

pub(crate) struct RequestActivityStart<'a> {
    pub(crate) trace_id: &'a str,
    pub(crate) client_ip: Option<&'a str>,
    pub(crate) key_id: &'a str,
    pub(crate) path: &'a str,
    pub(crate) method: &'a str,
    pub(crate) model: Option<&'a str>,
}

#[derive(Clone)]
struct RequestActivityEntry {
    id: String,
    trace_id: String,
    status: String,
    client_ip: Option<String>,
    key_id: String,
    path: String,
    method: String,
    model: Option<String>,
    route_kind: String,
    source_kind: Option<String>,
    source_id: Option<String>,
    created_at: Instant,
    created_at_ms: i64,
    queued_at: Option<Instant>,
    queued_at_ms: Option<i64>,
    running_at: Option<Instant>,
    running_at_ms: Option<i64>,
}

#[derive(Default)]
struct ActiveIpGroupBuilder {
    total_count: i64,
    queued_count: i64,
    running_count: i64,
    max_wait_ms: i64,
    max_running_ms: i64,
}

pub(crate) struct RequestActivityGuard {
    trace_id: String,
}

fn activity_table() -> &'static Mutex<HashMap<String, RequestActivityEntry>> {
    REQUEST_ACTIVITY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn clean_string(value: &str) -> String {
    value.trim().to_string()
}

fn clean_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(crate) fn begin_request_activity(start: RequestActivityStart<'_>) -> RequestActivityGuard {
    let trace_id = clean_string(start.trace_id);
    let now = Instant::now();
    let now_ms = now_ms();
    if trace_id.is_empty() {
        return RequestActivityGuard { trace_id };
    }
    let entry = RequestActivityEntry {
        id: trace_id.clone(),
        trace_id: trace_id.clone(),
        status: STATUS_QUEUED.to_string(),
        client_ip: clean_optional_string(start.client_ip),
        key_id: clean_string(start.key_id),
        path: clean_string(start.path),
        method: clean_string(start.method),
        model: clean_optional_string(start.model),
        route_kind: "gateway".to_string(),
        source_kind: None,
        source_id: None,
        created_at: now,
        created_at_ms: now_ms,
        queued_at: Some(now),
        queued_at_ms: Some(now_ms),
        running_at: None,
        running_at_ms: None,
    };
    let mut table = crate::lock_utils::lock_recover(activity_table(), "request_activity");
    table.insert(trace_id.clone(), entry);
    RequestActivityGuard { trace_id }
}

pub(crate) fn mark_request_activity_queued(trace_id: &str, route_kind: &str) {
    let mut table = crate::lock_utils::lock_recover(activity_table(), "request_activity");
    let Some(entry) = table.get_mut(trace_id) else {
        return;
    };
    let now = Instant::now();
    entry.status = STATUS_QUEUED.to_string();
    entry.route_kind = clean_string(route_kind);
    entry.queued_at = Some(now);
    entry.queued_at_ms = Some(now_ms());
    entry.running_at = None;
    entry.running_at_ms = None;
}

pub(crate) fn mark_request_activity_running(trace_id: &str, route_kind: &str) {
    let mut table = crate::lock_utils::lock_recover(activity_table(), "request_activity");
    let Some(entry) = table.get_mut(trace_id) else {
        return;
    };
    let now = Instant::now();
    entry.status = STATUS_RUNNING.to_string();
    entry.route_kind = clean_string(route_kind);
    if entry.queued_at.is_none() {
        entry.queued_at = Some(entry.created_at);
        entry.queued_at_ms = Some(entry.created_at_ms);
    }
    entry.running_at = Some(now);
    entry.running_at_ms = Some(now_ms());
}

pub(crate) fn update_request_activity_source(trace_id: &str, source_kind: &str, source_id: &str) {
    let mut table = crate::lock_utils::lock_recover(activity_table(), "request_activity");
    let Some(entry) = table.get_mut(trace_id) else {
        return;
    };
    entry.source_kind = clean_optional_string(Some(source_kind));
    entry.source_id = clean_optional_string(Some(source_id));
}

fn elapsed_ms(start: Option<Instant>, now: Instant) -> i64 {
    start
        .map(|instant| now.saturating_duration_since(instant).as_millis())
        .unwrap_or(0)
        .min(i64::MAX as u128) as i64
}

fn entry_wait_ms(entry: &RequestActivityEntry, now: Instant) -> i64 {
    match (entry.queued_at, entry.running_at) {
        (Some(queued_at), Some(running_at)) => running_at
            .saturating_duration_since(queued_at)
            .as_millis()
            .min(i64::MAX as u128) as i64,
        (Some(queued_at), None) => elapsed_ms(Some(queued_at), now),
        _ => 0,
    }
}

fn entry_running_ms(entry: &RequestActivityEntry, now: Instant) -> i64 {
    if entry.status == STATUS_RUNNING {
        elapsed_ms(entry.running_at, now)
    } else {
        0
    }
}

fn to_dashboard_item(entry: RequestActivityEntry, now: Instant) -> DashboardActiveRequestItem {
    let wait_ms = entry_wait_ms(&entry, now);
    let running_ms = entry_running_ms(&entry, now);
    DashboardActiveRequestItem {
        id: entry.id,
        trace_id: entry.trace_id,
        status: entry.status,
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
        wait_ms,
        running_ms,
    }
}

fn status_rank(status: &str) -> i32 {
    if status == STATUS_RUNNING {
        0
    } else {
        1
    }
}

pub(crate) fn request_activity_snapshot(limit: usize) -> DashboardActiveRequestsResult {
    let now = Instant::now();
    let entries = {
        let table = crate::lock_utils::lock_recover(activity_table(), "request_activity");
        table.values().cloned().collect::<Vec<_>>()
    };
    let total_count = entries.len() as i64;
    let running_count = entries
        .iter()
        .filter(|entry| entry.status == STATUS_RUNNING)
        .count() as i64;
    let queued_count = total_count.saturating_sub(running_count);

    let mut groups = BTreeMap::<String, ActiveIpGroupBuilder>::new();
    for entry in &entries {
        let Some(client_ip) = entry
            .client_ip
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let wait_ms = entry_wait_ms(entry, now);
        let running_ms = entry_running_ms(entry, now);
        let group = groups.entry(client_ip.to_string()).or_default();
        group.total_count += 1;
        if entry.status == STATUS_RUNNING {
            group.running_count += 1;
        } else {
            group.queued_count += 1;
        }
        group.max_wait_ms = group.max_wait_ms.max(wait_ms);
        group.max_running_ms = group.max_running_ms.max(running_ms);
    }
    let mut ip_groups = groups
        .into_iter()
        .map(|(client_ip, group)| DashboardActiveRequestIpGroup {
            client_ip,
            total_count: group.total_count,
            queued_count: group.queued_count,
            running_count: group.running_count,
            max_wait_ms: group.max_wait_ms,
            max_running_ms: group.max_running_ms,
        })
        .collect::<Vec<_>>();
    ip_groups.sort_by(|left, right| {
        right
            .total_count
            .cmp(&left.total_count)
            .then_with(|| right.running_count.cmp(&left.running_count))
            .then_with(|| left.client_ip.cmp(&right.client_ip))
    });

    let mut items = entries
        .into_iter()
        .map(|entry| to_dashboard_item(entry, now))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        status_rank(left.status.as_str())
            .cmp(&status_rank(right.status.as_str()))
            .then_with(|| left.created_at_ms.cmp(&right.created_at_ms))
            .then_with(|| left.trace_id.cmp(&right.trace_id))
    });
    items.truncate(limit.max(1));

    DashboardActiveRequestsResult {
        total_count,
        queued_count,
        running_count,
        items,
        ip_groups,
    }
}

impl Drop for RequestActivityGuard {
    fn drop(&mut self) {
        if self.trace_id.is_empty() {
            return;
        }
        let mut table = crate::lock_utils::lock_recover(activity_table(), "request_activity");
        table.remove(self.trace_id.as_str());
    }
}

#[cfg(test)]
pub(crate) fn clear_request_activity_for_tests() {
    let mut table = crate::lock_utils::lock_recover(activity_table(), "request_activity");
    table.clear();
}

#[cfg(test)]
mod tests {
    use super::{
        begin_request_activity, clear_request_activity_for_tests, mark_request_activity_running,
        request_activity_snapshot, RequestActivityStart,
    };

    #[test]
    fn snapshot_groups_running_and_queued_counts_by_client_ip() {
        clear_request_activity_for_tests();
        let _a = begin_request_activity(RequestActivityStart {
            trace_id: "a",
            client_ip: Some("192.168.1.20"),
            key_id: "key-a",
            path: "/v1/responses",
            method: "POST",
            model: Some("gpt-5"),
        });
        let _b = begin_request_activity(RequestActivityStart {
            trace_id: "b",
            client_ip: Some("192.168.1.20"),
            key_id: "key-b",
            path: "/v1/responses",
            method: "POST",
            model: Some("gpt-5"),
        });

        mark_request_activity_running("a", "client_ip_gate");

        let snapshot = request_activity_snapshot(50);
        let group = snapshot
            .ip_groups
            .iter()
            .find(|item| item.client_ip == "192.168.1.20")
            .expect("ip group");
        assert_eq!(group.running_count, 1);
        assert_eq!(group.queued_count, 1);
        assert_eq!(group.total_count, 2);

        clear_request_activity_for_tests();
    }

    #[test]
    fn dropping_activity_guard_removes_entry() {
        clear_request_activity_for_tests();
        {
            let _guard = begin_request_activity(RequestActivityStart {
                trace_id: "cleanup",
                client_ip: Some("192.168.1.21"),
                key_id: "key-a",
                path: "/v1/chat/completions",
                method: "POST",
                model: None,
            });
            assert_eq!(request_activity_snapshot(50).total_count, 1);
        }
        assert_eq!(request_activity_snapshot(50).total_count, 0);
        clear_request_activity_for_tests();
    }
}
