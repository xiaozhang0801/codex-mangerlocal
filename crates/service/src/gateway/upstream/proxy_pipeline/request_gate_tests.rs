use super::*;
use std::time::{Duration, Instant};

const ENV_REQUEST_GATE_WAIT_TIMEOUT_MS: &str = "CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS";

#[tokio::test(flavor = "current_thread")]
async fn account_gate_activity_seventh_request_waits_for_two_full_accounts() {
    use crate::gateway::{
        begin_request_activity, mark_request_activity_running, request_activity_snapshot,
        RequestActivityStart,
    };
    use std::future::Future;
    use std::task::Poll;

    let _guard = crate::test_env_guard();
    let previous_limit = crate::gateway::account_max_concurrent_limit();
    crate::gateway::set_account_max_concurrent_limit(3);
    let mut slots = Vec::new();
    let mut activities = Vec::new();
    for index in 0..7 {
        let trace = format!("activity-capacity-{index}");
        activities.push(begin_request_activity(RequestActivityStart {
            trace_id: &trace,
            client_ip: Some("192.168.1.10"),
            key_id: "activity-key",
            path: "/v1/responses",
            method: "POST",
            model: None,
        }));
        if index < 6 {
            let account = if index < 3 {
                "activity-account-a"
            } else {
                "activity-account-b"
            };
            slots.push(
                crate::gateway::account_request_gate_lock(account, 3)
                    .try_acquire()
                    .unwrap()
                    .unwrap(),
            );
        }
        // Reproduce the old IP-gate stage marking all seven requests as running.
        mark_request_activity_running(&trace, "account");
    }
    let waiting = acquire_account_request_gate_async(
        "activity-capacity-6",
        "activity-account-b",
        Some(Instant::now() + Duration::from_secs(5)),
    );
    tokio::pin!(waiting);
    std::future::poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let snapshot = request_activity_snapshot(50);
    let rows: Vec<_> = snapshot
        .items
        .iter()
        .filter(|item| item.trace_id.starts_with("activity-capacity-"))
        .collect();
    let running = rows.iter().filter(|item| item.status == "running").count();
    let queued = rows.iter().filter(|item| item.status == "queued").count();
    drop(slots.pop());
    let acquired = waiting
        .await
        .unwrap_or_else(|_| panic!("released account slot"));
    let after = request_activity_snapshot(50);
    let seventh = after
        .items
        .iter()
        .find(|item| item.trace_id == "activity-capacity-6")
        .unwrap();
    crate::gateway::set_account_max_concurrent_limit(previous_limit);
    assert_eq!((running, queued), (6, 1));
    assert_eq!(seventh.status, "running");
    drop(acquired);
    drop(activities);
}

fn restore_request_gate_wait_timeout(previous: Option<String>) {
    match previous {
        Some(value) => std::env::set_var(ENV_REQUEST_GATE_WAIT_TIMEOUT_MS, value),
        None => std::env::remove_var(ENV_REQUEST_GATE_WAIT_TIMEOUT_MS),
    }
    crate::gateway::reload_runtime_config_from_env();
}

#[tokio::test(flavor = "current_thread")]
async fn account_request_gate_times_out_when_the_account_is_at_capacity() {
    let _guard = crate::test_env_guard();
    let previous = std::env::var(ENV_REQUEST_GATE_WAIT_TIMEOUT_MS).ok();
    std::env::set_var(ENV_REQUEST_GATE_WAIT_TIMEOUT_MS, "30");
    crate::gateway::reload_runtime_config_from_env();
    crate::gateway::set_account_max_concurrent_limit(1);

    let lock = crate::gateway::account_request_gate_lock("account-gate-bounded", 1);
    let occupied = lock
        .try_acquire()
        .expect("lock should not be poisoned")
        .expect("first account request should acquire a slot");

    let started = Instant::now();
    let result = acquire_account_request_gate_async(
        "trc_account_gate_second",
        "account-gate-bounded",
        Some(Instant::now() + Duration::from_secs(5)),
    )
    .await;
    let waited = started.elapsed();

    drop(occupied);
    restore_request_gate_wait_timeout(previous);

    assert!(matches!(result, Err(AccountRequestGateError::Timeout)));
    assert!(waited >= Duration::from_millis(20));
    assert!(waited < Duration::from_millis(500));
}
