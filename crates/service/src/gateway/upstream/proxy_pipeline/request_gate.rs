use super::super::support::deadline;
use std::time::Instant;

pub(in super::super) enum AccountRequestGateError {
    Unavailable,
    Timeout,
    Cancelled,
}

pub(in super::super) async fn acquire_account_request_gate_async(
    trace_id: &str,
    account_id: &str,
    request_deadline: Option<Instant>,
) -> Result<super::super::super::RequestGateGuard, AccountRequestGateError> {
    let lock = super::super::super::account_request_gate_lock(
        account_id,
        super::super::super::account_max_concurrent_limit(),
    );
    let started = Instant::now();
    let timeout = match super::super::super::request_gate_wait_timeout() {
        Some(timeout) => deadline::cap_wait(timeout, request_deadline),
        None => deadline::remaining(request_deadline),
    };
    log::debug!(
        "event=account_request_gate_wait trace_id={} account_id={}",
        trace_id,
        account_id
    );

    let wait = async {
        if let Some(guard) = lock.try_acquire()? {
            return Ok(Some(guard));
        }
        match timeout {
            Some(timeout) => tokio::time::timeout(timeout, lock.acquire_async())
                .await
                .map_or(Ok(None), |result| result.map(Some)),
            None if deadline::is_expired(request_deadline) => Ok(None),
            None => lock.acquire_async().await.map(Some),
        }
    };
    match crate::http::gateway_request::with_response_cancellation(wait).await {
        Ok(Ok(Some(guard))) => {
            log::debug!(
                "event=account_request_gate_acquired trace_id={} account_id={} wait_ms={}",
                trace_id,
                account_id,
                started.elapsed().as_millis()
            );
            Ok(guard)
        }
        Ok(Err(_)) => {
            log::warn!(
                "event=account_request_gate_unavailable trace_id={} account_id={} wait_ms={}",
                trace_id,
                account_id,
                started.elapsed().as_millis()
            );
            Err(AccountRequestGateError::Unavailable)
        }
        Err(()) => Err(AccountRequestGateError::Cancelled),
        Ok(Ok(None)) => {
            let reason = if deadline::is_expired(request_deadline) {
                "total_timeout"
            } else {
                "gate_wait_timeout"
            };
            log::warn!(
                "event=account_request_gate_timeout trace_id={} account_id={} reason={} wait_ms={}",
                trace_id,
                account_id,
                reason,
                started.elapsed().as_millis()
            );
            Err(AccountRequestGateError::Timeout)
        }
    }
}

pub(in super::super) enum ClientIpRequestGateError {
    Unavailable,
    Timeout,
}

pub(in super::super) fn acquire_client_ip_request_gate(
    trace_id: &str,
    client_ip: Option<&str>,
    request_deadline: Option<Instant>,
) -> Result<Option<super::super::super::RequestGateGuard>, ClientIpRequestGateError> {
    let Some(client_ip) = client_ip.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let max_concurrent = if super::super::super::has_multiple_active_client_ips() {
        super::super::super::client_ip_multi_max_concurrent_limit()
    } else {
        super::super::super::client_ip_single_max_concurrent_limit()
    };
    let request_gate_lock = super::super::super::client_ip_gate_lock(client_ip, max_concurrent);
    let request_gate_wait_timeout = super::super::super::request_gate_wait_timeout();
    let gate_wait_started_at = Instant::now();
    log::debug!(
        "event=client_ip_request_gate_wait trace_id={} client_ip={} max_concurrent={}",
        trace_id,
        client_ip,
        max_concurrent
    );

    match request_gate_lock.try_acquire() {
        Ok(Some(guard)) => {
            log::debug!(
                "event=client_ip_request_gate_acquired trace_id={} client_ip={} wait_ms=0",
                trace_id,
                client_ip
            );
            Ok(Some(guard))
        }
        Ok(None) => {
            let wait_result = match request_gate_wait_timeout {
                Some(wait_timeout) => match deadline::cap_wait(wait_timeout, request_deadline) {
                    Some(effective_wait) if !effective_wait.is_zero() => {
                        request_gate_lock.acquire_with_timeout(effective_wait)
                    }
                    _ => Ok(None),
                },
                None => match deadline::remaining(request_deadline) {
                    Some(remaining) if remaining.is_zero() => Ok(None),
                    Some(remaining) => request_gate_lock.acquire_with_timeout(remaining),
                    None => request_gate_lock.acquire().map(Some),
                },
            };
            match wait_result {
                Ok(Some(guard)) => {
                    log::debug!(
                        "event=client_ip_request_gate_acquired trace_id={} client_ip={} wait_ms={}",
                        trace_id,
                        client_ip,
                        gate_wait_started_at.elapsed().as_millis()
                    );
                    Ok(Some(guard))
                }
                Err(super::super::super::RequestGateAcquireError::Poisoned) => {
                    log::warn!(
                        "event=client_ip_request_gate_unavailable trace_id={} client_ip={} wait_ms={}",
                        trace_id,
                        client_ip,
                        gate_wait_started_at.elapsed().as_millis()
                    );
                    Err(ClientIpRequestGateError::Unavailable)
                }
                Ok(None) => {
                    let reason = if deadline::is_expired(request_deadline) {
                        "total_timeout"
                    } else {
                        "gate_wait_timeout"
                    };
                    log::warn!(
                        "event=client_ip_request_gate_timeout trace_id={} client_ip={} reason={} wait_ms={}",
                        trace_id,
                        client_ip,
                        reason,
                        gate_wait_started_at.elapsed().as_millis()
                    );
                    Err(ClientIpRequestGateError::Timeout)
                }
            }
        }
        Err(super::super::super::RequestGateAcquireError::Poisoned) => {
            log::warn!(
                "event=client_ip_request_gate_unavailable trace_id={} client_ip={} wait_ms=0",
                trace_id,
                client_ip
            );
            Err(ClientIpRequestGateError::Unavailable)
        }
    }
}

#[cfg(test)]
#[path = "request_gate_tests.rs"]
mod tests;
