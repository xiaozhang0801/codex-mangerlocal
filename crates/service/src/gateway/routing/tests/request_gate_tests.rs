use super::*;
use std::thread;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "current_thread")]
async fn async_waiters_release_and_cancel_without_blocking_runtime() {
    let lock = Arc::new(RequestGateLock::new());
    let first = lock.try_acquire().unwrap().unwrap();
    let cancelled_lock = Arc::clone(&lock);
    let cancelled = tokio::spawn(async move { cancelled_lock.acquire_async().await });
    tokio::task::yield_now().await;
    cancelled.abort();
    assert!(matches!(cancelled.await, Err(error) if error.is_cancelled()));
    let waiting_lock = Arc::clone(&lock);
    let waiting = tokio::spawn(async move { waiting_lock.acquire_async().await });
    tokio::task::yield_now().await;
    assert!(!waiting.is_finished());
    drop(first);
    let next = tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .expect("waiter woken")
        .unwrap()
        .unwrap();
    assert!(lock.try_acquire().unwrap().is_none());
    drop(next);
    assert!(lock.try_acquire().unwrap().is_some());
}

/// 函数 `same_scope_reuses_same_lock_instance`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn same_account_reuses_same_lock_instance() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let first = account_request_gate_lock("account-1", 3);
    let second = account_request_gate_lock("account-1", 3);
    assert!(Arc::ptr_eq(&first, &second));
}

/// 函数 `different_scope_uses_different_lock_instances`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn different_accounts_use_different_lock_instances() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let first = account_request_gate_lock("account-1", 3);
    let second = account_request_gate_lock("account-2", 3);
    assert!(!Arc::ptr_eq(&first, &second));
}

/// 函数 `stale_unshared_lock_entry_is_reclaimed`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn stale_unshared_account_lock_entry_is_reclaimed() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let key = account_gate_key("account-1");
    let first = account_request_gate_lock("account-1", 3);
    let weak = Arc::downgrade(&first);
    drop(first);

    let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(RequestGateLockTable::default()));
    let mut table = lock.lock().expect("request gate table lock");
    let now = now_ts();
    table
        .entries
        .get_mut(&key)
        .expect("request gate entry")
        .last_seen_at = now - REQUEST_GATE_LOCK_TTL_SECS - 1;
    table.last_cleanup_at = now - REQUEST_GATE_LOCK_CLEANUP_INTERVAL_SECS - 1;
    drop(table);

    let _second = account_request_gate_lock("account-1", 3);
    assert!(weak.upgrade().is_none());
}

/// 函数 `stale_shared_lock_entry_is_not_reclaimed`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn stale_shared_account_lock_entry_is_not_reclaimed() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let key = account_gate_key("account-1");
    let first = account_request_gate_lock("account-1", 3);

    let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(RequestGateLockTable::default()));
    let mut table = lock.lock().expect("request gate table lock");
    let now = now_ts();
    table
        .entries
        .get_mut(&key)
        .expect("request gate entry")
        .last_seen_at = now - REQUEST_GATE_LOCK_TTL_SECS - 1;
    table.last_cleanup_at = now - REQUEST_GATE_LOCK_CLEANUP_INTERVAL_SECS - 1;
    drop(table);

    let second = account_request_gate_lock("account-1", 3);
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn acquire_waits_until_capacity_guard_released() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let lock = account_request_gate_lock("account-wait", 3);
    let mut occupied_guards = Vec::new();
    for _ in 0..3 {
        occupied_guards.push(
            lock.try_acquire()
                .expect("lock should not be poisoned")
                .expect("capacity guard"),
        );
    }
    let waiter = lock.clone();

    let handle = thread::spawn(move || {
        let started_at = Instant::now();
        let guard = waiter.acquire().expect("waiter acquires after release");
        let waited = started_at.elapsed();
        drop(guard);
        waited
    });

    thread::sleep(Duration::from_millis(60));
    drop(occupied_guards);

    let waited = handle.join().expect("join waiter thread");
    assert!(
        waited >= Duration::from_millis(40),
        "expected waiter to block, actual wait: {waited:?}"
    );
}

#[test]
fn account_request_gate_limits_one_account_to_three_parallel_requests() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let lock = account_request_gate_lock("account-parallel", 3);
    let mut guards = Vec::new();

    for _ in 0..3 {
        guards.push(
            lock.try_acquire()
                .expect("lock should not be poisoned")
                .expect("same account should allow three parallel requests"),
        );
    }
    assert!(
        lock.try_acquire()
            .expect("lock should not be poisoned")
            .is_none(),
        "same account should queue the fourth request"
    );
}

#[test]
fn lock_with_max_running_allows_configured_parallel_guards() {
    let _guard = crate::test_env_guard();
    let lock = Arc::new(RequestGateLock::new().with_max_running(2));

    let first = lock
        .try_acquire()
        .expect("lock should not be poisoned")
        .expect("first guard");
    let second = lock
        .try_acquire()
        .expect("lock should not be poisoned")
        .expect("second guard");
    assert!(
        lock.try_acquire()
            .expect("lock should not be poisoned")
            .is_none(),
        "third guard should wait while max running is reached"
    );

    drop(first);
    let third = lock
        .try_acquire()
        .expect("lock should not be poisoned")
        .expect("slot released");
    drop(second);
    drop(third);
}

#[test]
fn updating_an_account_limit_updates_the_existing_lock() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let lock = account_request_gate_lock("account-dynamic", 3);
    let mut guards = Vec::new();
    for _ in 0..3 {
        guards.push(lock.try_acquire().unwrap().expect("account slot"));
    }
    assert!(lock.try_acquire().unwrap().is_none());

    let same_lock = account_request_gate_lock("account-dynamic", 4);
    assert!(Arc::ptr_eq(&lock, &same_lock));
    let fourth = lock.try_acquire().unwrap().expect("expanded account slot");
    drop(fourth);
    drop(guards);
}

#[test]
fn client_ip_gate_uses_the_configured_single_and_multi_ip_limits() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let lock = client_ip_gate_lock("192.168.1.20", 3);
    let mut guards = Vec::new();

    for _ in 0..3 {
        guards.push(
            lock.try_acquire()
                .expect("lock should not be poisoned")
                .expect("ip slot"),
        );
    }
    assert!(
        lock.try_acquire()
            .expect("lock should not be poisoned")
            .is_none(),
        "single active IP should be queued after three slots"
    );

    let same_lock = client_ip_gate_lock("192.168.1.20", 2);
    assert!(Arc::ptr_eq(&lock, &same_lock));
    assert!(
        same_lock.try_acquire().unwrap().is_none(),
        "multiple active IPs must lower each IP to two slots"
    );

    let other_ip_lock = client_ip_gate_lock("192.168.1.21", 2);
    let first_other_ip_guard = other_ip_lock.try_acquire().unwrap().expect("other IP slot");
    let second_other_ip_guard = other_ip_lock.try_acquire().unwrap().expect("other IP slot");
    assert!(other_ip_lock.try_acquire().unwrap().is_none());

    drop(guards.pop());
    assert!(lock.try_acquire().unwrap().is_none());
    drop(guards);
    drop(first_other_ip_guard);
    drop(second_other_ip_guard);
}
