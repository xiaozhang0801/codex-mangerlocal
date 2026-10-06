use super::*;
use std::time::{Duration, Instant};

const ENV_REQUEST_GATE_WAIT_TIMEOUT_MS: &str = "CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS";

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
