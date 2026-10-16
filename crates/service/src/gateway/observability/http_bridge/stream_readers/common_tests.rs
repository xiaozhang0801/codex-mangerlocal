use super::{
    classify_upstream_stream_read_error, stream_incomplete_message,
    stream_reader_disconnected_message,
};

#[test]
fn terminal_delivery_detects_split_marker_before_large_payload() {
    let collector = super::Arc::new(super::Mutex::new(super::PassthroughSseCollector::default()));
    let mut tracker = super::TerminalDeliveryTracker::default();
    super::mark_terminal_delivered(&collector, &mut tracker, b"event: response.fa");
    assert!(!collector.lock().unwrap().terminal_delivered);
    let remainder = format!("iled\ndata: {}\n\n", "x".repeat(1024));
    super::mark_terminal_delivered(&collector, &mut tracker, remainder.as_bytes());
    assert!(collector.lock().unwrap().terminal_delivered);
}

#[test]
fn terminal_delivery_requires_dispatched_event_not_payload_words() {
    let collector = super::Arc::new(super::Mutex::new(super::PassthroughSseCollector::default()));
    let mut tracker = super::TerminalDeliveryTracker::default();
    for chunk in [
        b": response.completed\n\n".as_slice(),
        b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"response.failed\"}\n\n",
        b"event: response.completed\ndata: {}\n",
    ] {
        super::mark_terminal_delivered(&collector, &mut tracker, chunk);
        assert!(!collector.lock().unwrap().terminal_delivered);
    }
    super::mark_terminal_delivered(&collector, &mut tracker, b"\n");
    assert!(collector.lock().unwrap().terminal_delivered);
}

#[test]
fn terminal_delivery_supports_data_only_and_crlf_split_at_every_byte() {
    for frame in [
        "data: {\"response\":{},\"type\":\"response.completed\"}\r\n\r\n",
        "data: [DONE]\n\n",
        "event: response.incomplete\r\ndata: {}\r\n\r\n",
    ] {
        let collector =
            super::Arc::new(super::Mutex::new(super::PassthroughSseCollector::default()));
        let mut tracker = super::TerminalDeliveryTracker::default();
        for chunk in frame.as_bytes().chunks(1) {
            super::mark_terminal_delivered(&collector, &mut tracker, chunk);
        }
        assert!(collector.lock().unwrap().terminal_delivered, "{frame}");
    }
}

#[test]
fn comment_keepalive_is_not_a_semantic_event() {
    let frame = super::SseKeepAliveFrame::Comment.bytes();
    assert_eq!(frame, b": keep-alive\n\n");
    assert!(!frame.starts_with(b"data:"));
    assert!(!String::from_utf8_lossy(frame).contains("codexmanager.keepalive"));
}

#[tokio::test(flavor = "current_thread")]
async fn terminal_delivery_is_immediate_with_exhausted_task_budget() {
    let collector = super::Arc::new(super::Mutex::new(super::PassthroughSseCollector::default()));
    let mut tracker = super::TerminalDeliveryTracker::default();
    while tokio::task::coop::has_budget_remaining() {
        tokio::task::consume_budget().await;
    }
    super::mark_terminal_delivered(
        &collector,
        &mut tracker,
        b"event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n",
    );
    assert!(collector.lock().unwrap().terminal_delivered);
}

/// 函数 `classify_upstream_stream_read_error_maps_body_error`
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
fn classify_upstream_stream_read_error_maps_body_error() {
    assert_eq!(
        classify_upstream_stream_read_error("request or response body error"),
        "上游中途断开，未返回具体错误信息"
    );
}

/// 函数 `classify_upstream_stream_read_error_maps_disconnect`
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
fn classify_upstream_stream_read_error_maps_disconnect() {
    assert_eq!(
        classify_upstream_stream_read_error("connection reset by peer"),
        "连接中断（可能是网络波动或客户端主动取消）"
    );
}

/// 函数 `classify_upstream_stream_read_error_maps_timeout`
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
fn classify_upstream_stream_read_error_maps_timeout() {
    assert_eq!(
        classify_upstream_stream_read_error("operation timed out"),
        "上游流式空闲超时"
    );
}

/// 函数 `stream_terminal_messages_are_user_friendly`
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
fn stream_terminal_messages_are_user_friendly() {
    assert_eq!(
        stream_incomplete_message(),
        "连接中断（可能是网络波动或客户端主动取消）"
    );
    assert_eq!(
        stream_reader_disconnected_message(),
        "连接中断（可能是网络波动或客户端主动取消）"
    );
    assert_eq!(super::stream_idle_timeout_message(), "上游流式空闲超时");
}
