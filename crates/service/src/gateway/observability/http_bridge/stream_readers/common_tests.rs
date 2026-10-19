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

#[test]
fn terminal_delivery_ignores_terminal_words_outside_complete_control_lines() {
    let mut tracker = super::TerminalDeliveryTracker::default();
    let collector = super::Arc::new(super::Mutex::new(super::PassthroughSseCollector::default()));
    for frame in [
        ": event: response.completed\n\n",
        "id: response.failed\n\n",
        "event: response.incomplete_detail\n\n",
        "event: response.completed trailing text\n\n",
        "Event: response.completed\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"response.done and data: [DONE]\"}\n\n",
    ] {
        for byte in frame.bytes() {
            super::mark_terminal_delivered(&collector, &mut tracker, &[byte]);
            assert!(!collector.lock().unwrap().terminal_delivered);
        }
    }
    super::mark_terminal_delivered(&collector, &mut tracker, b"event: response.completed");
    assert!(!collector.lock().unwrap().terminal_delivered);
    super::mark_terminal_delivered(&collector, &mut tracker, b"_detail\n\n");
    assert!(!collector.lock().unwrap().terminal_delivered);
    super::mark_terminal_delivered(&collector, &mut tracker, b"data: [DONE]\r\r");
    // The streaming parser waits for the possible LF following a trailing CR.
    assert!(!collector.lock().unwrap().terminal_delivered);
    super::mark_terminal_delivered(&collector, &mut tracker, b"\n");
    assert!(collector.lock().unwrap().terminal_delivered);
}

#[test]
fn terminal_delivery_accepts_control_fields_without_an_optional_space() {
    for frame in [
        b"event:response.completed\rdata: {}\r\r".as_slice(),
        b"data:[DONE]\n\n".as_slice(),
    ] {
        let mut tracker = super::TerminalDeliveryTracker::default();
        let collector =
            super::Arc::new(super::Mutex::new(super::PassthroughSseCollector::default()));
        for &byte in frame {
            super::mark_terminal_delivered(&collector, &mut tracker, &[byte]);
        }
        if frame.ends_with(b"\r") {
            assert!(!collector.lock().unwrap().terminal_delivered);
            super::mark_terminal_delivered(&collector, &mut tracker, b"\n");
        }
        assert!(collector.lock().unwrap().terminal_delivered);
    }
}

#[test]
fn terminal_delivery_does_not_treat_a_long_data_line_suffix_as_an_event() {
    let mut tracker = super::TerminalDeliveryTracker::default();
    let collector = super::Arc::new(super::Mutex::new(super::PassthroughSseCollector::default()));
    let data = format!("data: {}", "x".repeat(1024));
    super::mark_terminal_delivered(&collector, &mut tracker, data.as_bytes());
    super::mark_terminal_delivered(
        &collector,
        &mut tracker,
        b"event: response.failed\ndata: {}\n\n",
    );
    assert!(!collector.lock().unwrap().terminal_delivered);
    super::mark_terminal_delivered(
        &collector,
        &mut tracker,
        b"event: response.failed\ndata: {}\n\n",
    );
    assert!(collector.lock().unwrap().terminal_delivered);
}

#[test]
fn anthropic_terminal_delivery_detects_markers_split_across_small_reads() {
    for event in ["message_stop", "error"] {
        let collector =
            super::Arc::new(super::Mutex::new(super::PassthroughSseCollector::default()));
        let frame = format!("event: {event}\ndata: {{\"type\":\"{event}\"}}\n\n");
        for byte in frame.bytes() {
            super::mark_anthropic_terminal_delivered(&collector, &[byte]);
        }
        assert!(collector.lock().unwrap().terminal_delivered);
    }
}

#[test]
fn anthropic_terminal_delivery_accepts_cr_only_line_endings() {
    for event in ["message_stop", "error"] {
        let collector =
            super::Arc::new(super::Mutex::new(super::PassthroughSseCollector::default()));
        let frame = format!("event: {event}\rdata: {{\"type\":\"{event}\"}}\r\r");
        for byte in frame.bytes() {
            super::mark_anthropic_terminal_delivered(&collector, &[byte]);
        }
        assert!(collector.lock().unwrap().terminal_delivered);
    }
}

#[test]
fn anthropic_keepalive_and_content_events_are_not_terminal_delivery() {
    let collector = super::Arc::new(super::Mutex::new(super::PassthroughSseCollector::default()));
    for frame in [
        super::SseKeepAliveFrame::Anthropic.bytes(),
        b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\"}\n\n",
        b"event: error_detail\ndata: {}\n\n",
    ] {
        super::mark_anthropic_terminal_delivered(&collector, frame);
        assert!(!collector.lock().unwrap().terminal_delivered);
    }
}

#[test]
fn responses_generic_error_still_requires_normalized_terminal_delivery() {
    let mut tracker = super::TerminalDeliveryTracker::default();
    let collector = super::Arc::new(super::Mutex::new(super::PassthroughSseCollector::default()));
    super::mark_terminal_delivered(
        &collector,
        &mut tracker,
        b"event: error\ndata: {\"type\":\"error\",\"error\":{\"message\":\"upstream error\"}}\n\n",
    );
    assert!(!collector.lock().unwrap().terminal_delivered);
    super::mark_terminal_delivered(
        &collector,
        &mut tracker,
        b"event: response.failed\ndata: {\"type\":\"response.failed\"}\n\n",
    );
    assert!(collector.lock().unwrap().terminal_delivered);
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
