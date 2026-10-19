use super::{
    inspect_openai_responses_sse_frame, inspect_sse_frame, inspect_sse_frame_for_protocol,
    PassthroughSseProtocol, SseTerminal,
};

#[test]
fn first_line_bom_is_ignored_for_event_name_and_data_payload() {
    let header = vec![
        "\u{feff}event: response.completed\r\n".to_string(),
        "data: {\"response\":{\"id\":\"resp_bom_header\"}}\r\n".to_string(),
        "\r\n".to_string(),
    ];
    let data = vec![
        "\u{feff}data: {\"type\":\"response.completed\"}\r\n".to_string(),
        "\r\n".to_string(),
    ];

    let value = super::parse_sse_frame_json(&header).expect("BOM event frame");
    assert_eq!(value["type"], "response.completed");
    assert_eq!(value["response"]["id"], "resp_bom_header");
    assert_eq!(
        super::parse_sse_frame_json(&data).expect("BOM data frame")["type"],
        "response.completed"
    );
    assert!(header[0].starts_with('\u{feff}'));
    assert!(data[0].starts_with('\u{feff}'));
}

/// 函数 `inspect_sse_frame_keeps_last_event_type_from_header`
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
fn inspect_sse_frame_keeps_last_event_type_from_header() {
    let lines = vec![
        "event: response.completed\n".to_string(),
        "data: {\"type\":\"response.completed\"}\n".to_string(),
        "\n".to_string(),
    ];
    let inspection = inspect_sse_frame(&lines);
    assert_eq!(
        inspection.last_event_type.as_deref(),
        Some("response.completed")
    );
}

/// 函数 `inspect_sse_frame_keeps_last_event_type_from_json_type`
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
fn inspect_sse_frame_keeps_last_event_type_from_json_type() {
    let lines = vec![
        "data: {\"type\":\"response.failed\",\"error\":{\"message\":\"oops\"}}\n".to_string(),
        "\n".to_string(),
    ];
    let inspection = inspect_sse_frame(&lines);
    assert_eq!(
        inspection.last_event_type.as_deref(),
        Some("response.failed")
    );
}

#[test]
fn inspect_sse_frame_generic_mode_does_not_treat_message_stop_as_terminal() {
    let lines = vec![
        "event: message_stop\n".to_string(),
        "data: {\"type\":\"message_stop\"}\n".to_string(),
        "\n".to_string(),
    ];
    let inspection = inspect_sse_frame_for_protocol(&lines, PassthroughSseProtocol::Generic);
    assert!(inspection.terminal.is_none());
    assert_eq!(inspection.last_event_type.as_deref(), Some("message_stop"));
}

#[test]
fn inspect_sse_frame_anthropic_native_treats_message_stop_as_terminal() {
    let lines = vec![
        "event: message_stop\n".to_string(),
        "data: {\"type\":\"message_stop\"}\n".to_string(),
        "\n".to_string(),
    ];
    let inspection =
        inspect_sse_frame_for_protocol(&lines, PassthroughSseProtocol::AnthropicNative);
    assert!(matches!(inspection.terminal, Some(SseTerminal::Ok)));
    assert_eq!(inspection.last_event_type.as_deref(), Some("message_stop"));
}

#[test]
fn inspect_openai_responses_sse_frame_collects_output_item_text() {
    let lines = vec![
        "event: response.output_item.done\n".to_string(),
        "data: {\"output_item\":{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello from output_item\"}]}}\n".to_string(),
        "\n".to_string(),
    ];
    let inspection = inspect_openai_responses_sse_frame(&lines);
    let usage = inspection.usage.expect("usage");
    assert_eq!(usage.output_text.as_deref(), Some("hello from output_item"));
    assert_eq!(
        inspection.last_event_type.as_deref(),
        Some("response.output_item.done")
    );
}

#[test]
fn inspect_openai_responses_sse_frame_collects_structured_delta_text() {
    let lines = vec![
        "event: response.output_text.delta\n".to_string(),
        "data: {\"delta\":{\"text\":\"hello from structured delta\"}}\n".to_string(),
        "\n".to_string(),
    ];
    let inspection = inspect_openai_responses_sse_frame(&lines);
    let usage = inspection.usage.expect("usage");
    assert_eq!(
        usage.output_text.as_deref(),
        Some("hello from structured delta")
    );
    assert_eq!(
        inspection.last_event_type.as_deref(),
        Some("response.output_text.delta")
    );
}
