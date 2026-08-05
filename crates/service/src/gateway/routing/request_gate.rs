use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use codexmanager_core::storage::now_ts;

const REQUEST_GATE_LOCK_TTL_SECS: i64 = 30 * 60;
const REQUEST_GATE_LOCK_CLEANUP_INTERVAL_SECS: i64 = 60;
const CLIENT_IP_GATE_MAX_RUNNING: usize = 4;

struct RequestGateLockEntry {
    lock: Arc<RequestGateLock>,
    last_seen_at: i64,
}

#[derive(Default)]
struct RequestGateLockTable {
    entries: HashMap<String, RequestGateLockEntry>,
    last_cleanup_at: i64,
}

static REQUEST_GATE_LOCKS: OnceLock<Mutex<RequestGateLockTable>> = OnceLock::new();
static CLIENT_IP_GATE_STATE: OnceLock<ClientIpGateTable> = OnceLock::new();

#[derive(Debug)]
pub(crate) enum RequestGateAcquireError {
    Poisoned,
}

#[derive(Default)]
struct RequestGateState {
    running: usize,
}

#[derive(Default)]
struct ClientIpGateState {
    running_by_ip: HashMap<String, usize>,
}

#[derive(Default)]
struct ClientIpGateTable {
    state: Mutex<ClientIpGateState>,
    available: Condvar,
}

enum RequestGateMode {
    Fixed,
    ClientIp { client_ip: String },
}

pub(crate) struct RequestGateLock {
    state: Mutex<RequestGateState>,
    available: Condvar,
    max_running: usize,
    mode: RequestGateMode,
}

impl RequestGateLock {
    /// 函数 `new`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 返回函数执行结果
    fn new() -> Self {
        Self {
            state: Mutex::new(RequestGateState::default()),
            available: Condvar::new(),
            max_running: 1,
            mode: RequestGateMode::Fixed,
        }
    }

    fn for_client_ip(client_ip: &str) -> Self {
        let mut lock = Self::new().with_max_running(CLIENT_IP_GATE_MAX_RUNNING);
        lock.mode = RequestGateMode::ClientIp {
            client_ip: client_ip.trim().to_string(),
        };
        lock
    }

    fn with_max_running(mut self, max_running: usize) -> Self {
        self.max_running = max_running.max(1);
        self
    }

    /// 函数 `try_acquire`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - crate: 参数 crate
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(crate) fn try_acquire(
        self: &Arc<Self>,
    ) -> Result<Option<RequestGateGuard>, RequestGateAcquireError> {
        if let RequestGateMode::ClientIp { client_ip } = &self.mode {
            return try_acquire_client_ip_slot(client_ip, self.max_running).map(|slot| {
                slot.map(|_| RequestGateGuard {
                    lock: Arc::clone(self),
                })
            });
        }
        let mut state = match self.state.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::warn!("event=lock_poisoned lock=request_gate_state action=skip");
                return Err(RequestGateAcquireError::Poisoned);
            }
        };
        if state.running >= self.max_running {
            return Ok(None);
        }
        state.running += 1;
        drop(state);
        Ok(Some(RequestGateGuard {
            lock: Arc::clone(self),
        }))
    }

    pub(crate) fn acquire(self: &Arc<Self>) -> Result<RequestGateGuard, RequestGateAcquireError> {
        if let RequestGateMode::ClientIp { client_ip } = &self.mode {
            acquire_client_ip_slot(client_ip, self.max_running)?;
            return Ok(RequestGateGuard {
                lock: Arc::clone(self),
            });
        }
        let state = match self.state.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::warn!("event=lock_poisoned lock=request_gate_state action=skip_wait");
                return Err(RequestGateAcquireError::Poisoned);
            }
        };
        let max_running = self.max_running;
        let Ok(mut state) = self
            .available
            .wait_while(state, |state| state.running >= max_running)
        else {
            log::warn!("event=lock_poisoned lock=request_gate_state action=skip_wait_while");
            return Err(RequestGateAcquireError::Poisoned);
        };
        state.running += 1;
        drop(state);
        Ok(RequestGateGuard {
            lock: Arc::clone(self),
        })
    }

    /// 函数 `acquire_with_timeout`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - crate: 参数 crate
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(crate) fn acquire_with_timeout(
        self: &Arc<Self>,
        timeout: Duration,
    ) -> Result<Option<RequestGateGuard>, RequestGateAcquireError> {
        if let RequestGateMode::ClientIp { client_ip } = &self.mode {
            return acquire_client_ip_slot_with_timeout(client_ip, self.max_running, timeout).map(
                |slot| {
                    slot.map(|_| RequestGateGuard {
                        lock: Arc::clone(self),
                    })
                },
            );
        }
        let state = match self.state.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::warn!("event=lock_poisoned lock=request_gate_state action=skip_wait");
                return Err(RequestGateAcquireError::Poisoned);
            }
        };
        let wait_result = self
            .available
            .wait_timeout_while(state, timeout, |state| state.running >= self.max_running);
        let Ok((mut state, _)) = wait_result else {
            log::warn!("event=lock_poisoned lock=request_gate_state action=skip_wait_timeout");
            return Err(RequestGateAcquireError::Poisoned);
        };
        if state.running >= self.max_running {
            return Ok(None);
        }
        state.running += 1;
        drop(state);
        Ok(Some(RequestGateGuard {
            lock: Arc::clone(self),
        }))
    }

    fn release(&self) {
        if let RequestGateMode::ClientIp { client_ip } = &self.mode {
            release_client_ip_slot(client_ip);
            return;
        }
        let mut state = match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::warn!("event=lock_poisoned lock=request_gate_state action=recover_release");
                poisoned.into_inner()
            }
        };
        state.running = state.running.saturating_sub(1);
        self.available.notify_one();
    }
}

pub(crate) struct RequestGateGuard {
    lock: Arc<RequestGateLock>,
}

impl Drop for RequestGateGuard {
    /// 函数 `drop`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 无
    fn drop(&mut self) {
        self.lock.release();
    }
}

/// 函数 `gate_key`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - key_id: 参数 key_id
/// - path: 参数 path
/// - model: 参数 model
///
/// # 返回
/// 返回函数执行结果
fn gate_key(key_id: &str, path: &str, model: Option<&str>) -> String {
    format!(
        "{}|{}|{}",
        key_id.trim(),
        path.trim(),
        model
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("-")
    )
}

fn client_ip_gate_key(client_ip: &str) -> String {
    format!("client_ip|{}", client_ip.trim())
}

fn client_ip_gate_table() -> &'static ClientIpGateTable {
    CLIENT_IP_GATE_STATE.get_or_init(ClientIpGateTable::default)
}

fn client_ip_gate_can_acquire(
    state: &ClientIpGateState,
    client_ip: &str,
    max_running: usize,
) -> bool {
    let running_for_ip = state.running_by_ip.get(client_ip).copied().unwrap_or(0);
    let active_ip_count = state
        .running_by_ip
        .values()
        .filter(|running| **running > 0)
        .count();
    let active_ip_count_after = if running_for_ip > 0 {
        active_ip_count
    } else {
        active_ip_count.saturating_add(1)
    };
    active_ip_count_after <= 1 || running_for_ip < max_running.max(1)
}

fn record_client_ip_acquire(state: &mut ClientIpGateState, client_ip: &str) {
    *state
        .running_by_ip
        .entry(client_ip.to_string())
        .or_default() += 1;
}

fn try_acquire_client_ip_slot(
    client_ip: &str,
    max_running: usize,
) -> Result<Option<()>, RequestGateAcquireError> {
    let table = client_ip_gate_table();
    let mut state = match table.state.lock() {
        Ok(guard) => guard,
        Err(_) => {
            log::warn!("event=lock_poisoned lock=client_ip_gate_state action=skip");
            return Err(RequestGateAcquireError::Poisoned);
        }
    };
    if !client_ip_gate_can_acquire(&state, client_ip, max_running) {
        return Ok(None);
    }
    record_client_ip_acquire(&mut state, client_ip);
    Ok(Some(()))
}

fn acquire_client_ip_slot(
    client_ip: &str,
    max_running: usize,
) -> Result<(), RequestGateAcquireError> {
    let table = client_ip_gate_table();
    let state = match table.state.lock() {
        Ok(guard) => guard,
        Err(_) => {
            log::warn!("event=lock_poisoned lock=client_ip_gate_state action=skip_wait");
            return Err(RequestGateAcquireError::Poisoned);
        }
    };
    let max_running = max_running.max(1);
    let Ok(mut state) = table.available.wait_while(state, |state| {
        !client_ip_gate_can_acquire(state, client_ip, max_running)
    }) else {
        log::warn!("event=lock_poisoned lock=client_ip_gate_state action=skip_wait_while");
        return Err(RequestGateAcquireError::Poisoned);
    };
    record_client_ip_acquire(&mut state, client_ip);
    Ok(())
}

fn acquire_client_ip_slot_with_timeout(
    client_ip: &str,
    max_running: usize,
    timeout: Duration,
) -> Result<Option<()>, RequestGateAcquireError> {
    let table = client_ip_gate_table();
    let state = match table.state.lock() {
        Ok(guard) => guard,
        Err(_) => {
            log::warn!("event=lock_poisoned lock=client_ip_gate_state action=skip_wait");
            return Err(RequestGateAcquireError::Poisoned);
        }
    };
    let max_running = max_running.max(1);
    let wait_result = table.available.wait_timeout_while(state, timeout, |state| {
        !client_ip_gate_can_acquire(state, client_ip, max_running)
    });
    let Ok((mut state, _)) = wait_result else {
        log::warn!("event=lock_poisoned lock=client_ip_gate_state action=skip_wait_timeout");
        return Err(RequestGateAcquireError::Poisoned);
    };
    if !client_ip_gate_can_acquire(&state, client_ip, max_running) {
        return Ok(None);
    }
    record_client_ip_acquire(&mut state, client_ip);
    Ok(Some(()))
}

fn release_client_ip_slot(client_ip: &str) {
    let table = client_ip_gate_table();
    let mut state = match table.state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::warn!("event=lock_poisoned lock=client_ip_gate_state action=recover_release");
            poisoned.into_inner()
        }
    };
    let should_remove = if let Some(running) = state.running_by_ip.get_mut(client_ip) {
        *running = running.saturating_sub(1);
        *running == 0
    } else {
        false
    };
    if should_remove {
        state.running_by_ip.remove(client_ip);
    }
    table.available.notify_all();
}

fn clear_client_ip_gate_state() {
    let Some(table) = CLIENT_IP_GATE_STATE.get() else {
        return;
    };
    let mut state = crate::lock_utils::lock_recover(&table.state, "client_ip_gate_state");
    state.running_by_ip.clear();
    table.available.notify_all();
}

fn gate_lock_for_key<F>(key: String, create_lock: F) -> Arc<RequestGateLock>
where
    F: FnOnce() -> RequestGateLock,
{
    let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(RequestGateLockTable::default()));
    let mut table = crate::lock_utils::lock_recover(lock, "request_gate_locks");
    let now = now_ts();
    maybe_cleanup_request_gate_locks(&mut table, now);
    let entry = table
        .entries
        .entry(key)
        .or_insert_with(|| RequestGateLockEntry {
            lock: Arc::new(create_lock()),
            last_seen_at: now,
        });
    entry.last_seen_at = now;
    entry.lock.clone()
}

/// 函数 `request_gate_lock`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn request_gate_lock(
    key_id: &str,
    path: &str,
    model: Option<&str>,
) -> Arc<RequestGateLock> {
    gate_lock_for_key(gate_key(key_id, path, model), || RequestGateLock::new())
}

pub(crate) fn client_ip_gate_lock(client_ip: &str) -> Arc<RequestGateLock> {
    gate_lock_for_key(client_ip_gate_key(client_ip), || {
        RequestGateLock::for_client_ip(client_ip)
    })
}

/// 函数 `maybe_cleanup_request_gate_locks`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - table: 参数 table
/// - now: 参数 now
///
/// # 返回
/// 无
fn maybe_cleanup_request_gate_locks(table: &mut RequestGateLockTable, now: i64) {
    if table.last_cleanup_at != 0
        && now.saturating_sub(table.last_cleanup_at) < REQUEST_GATE_LOCK_CLEANUP_INTERVAL_SECS
    {
        return;
    }
    table.last_cleanup_at = now;
    table.entries.retain(|_, entry| {
        let stale = now.saturating_sub(entry.last_seen_at) > REQUEST_GATE_LOCK_TTL_SECS;
        !stale || Arc::strong_count(&entry.lock) > 1
    });
}

/// 函数 `clear_runtime_state`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn clear_runtime_state() {
    let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(RequestGateLockTable::default()));
    let mut table = crate::lock_utils::lock_recover(lock, "request_gate_locks");
    table.entries.clear();
    table.last_cleanup_at = 0;
    drop(table);
    clear_client_ip_gate_state();
}

/// 函数 `clear_request_gate_locks_for_tests`
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
#[cfg(test)]
fn clear_request_gate_locks_for_tests() {
    clear_runtime_state();
}

#[cfg(test)]
#[path = "tests/request_gate_tests.rs"]
mod tests;
