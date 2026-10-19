use super::super::{
    AnthropicSseReader, ChatCompletionsFromResponsesSseReader, GeminiSseReader,
    ImagesFromResponsesSseReader, ImagesResponseFormat, PassthroughSseCollector,
    PassthroughSseProtocol, PassthroughSseUsageReader, ResponsesFromAnthropicSseReader,
    SseKeepAliveFrame, UpstreamResponseUsage,
};
use super::{SseFrameLimits, UpstreamSseFramePump};
use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem};
use crate::gateway::GeminiStreamOutputMode;
use bytes::Bytes;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::io::{self, Read};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::Semaphore;

#[derive(Clone, Copy, Debug)]
enum ReaderKind {
    Responses,
    Chat,
    Anthropic,
    AnthropicNative,
    Gemini,
    ResponsesFromAnthropic,
    Images,
}

const ALL_READERS: [ReaderKind; 7] = [
    ReaderKind::Responses,
    ReaderKind::Chat,
    ReaderKind::Anthropic,
    ReaderKind::AnthropicNative,
    ReaderKind::Gemini,
    ReaderKind::ResponsesFromAnthropic,
    ReaderKind::Images,
];

fn pump(
    body: &str,
    chunk_bytes: usize,
    frame_bytes: usize,
    shared_bytes: usize,
) -> UpstreamSseFramePump {
    let chunks = body.as_bytes().chunks(chunk_bytes).collect::<Vec<_>>();
    let (tx, rx) = tokio::sync::mpsc::channel(chunks.len() + 1);
    for chunk in chunks {
        tx.try_send(GatewayByteStreamItem::Chunk(Bytes::copy_from_slice(chunk)))
            .expect("queue fixture chunk");
    }
    tx.try_send(GatewayByteStreamItem::Eof)
        .expect("queue fixture EOF");
    UpstreamSseFramePump::from_stream_with_limits(
        GatewayByteStream::from_receiver(rx),
        SseFrameLimits {
            frame_bytes,
            queued_bytes: frame_bytes * 2,
        },
        Arc::new(Semaphore::new(shared_bytes)),
    )
}

fn reader(kind: ReaderKind, pump: UpstreamSseFramePump) -> Box<dyn Read> {
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let started_at = Instant::now();
    match kind {
        ReaderKind::Responses | ReaderKind::AnthropicNative => {
            let (keepalive, protocol) = match kind {
                ReaderKind::AnthropicNative => (
                    SseKeepAliveFrame::Anthropic,
                    PassthroughSseProtocol::AnthropicNative,
                ),
                _ => (SseKeepAliveFrame::Comment, PassthroughSseProtocol::Generic),
            };
            Box::new(PassthroughSseUsageReader::from_pump(
                pump, collector, keepalive, protocol, started_at,
            ))
        }
        ReaderKind::Chat => Box::new(ChatCompletionsFromResponsesSseReader::from_pump(
            pump, collector, started_at,
        )),
        ReaderKind::Anthropic => Box::new(AnthropicSseReader::from_pump(
            pump,
            collector,
            Some("fixture"),
            None,
            started_at,
        )),
        ReaderKind::Gemini => Box::new(GeminiSseReader::from_pump(
            pump,
            collector,
            None,
            GeminiStreamOutputMode::Sse,
            false,
            started_at,
        )),
        ReaderKind::ResponsesFromAnthropic => Box::new(ResponsesFromAnthropicSseReader::from_pump(
            pump,
            Arc::new(Mutex::new(UpstreamResponseUsage::default())),
            Some("fixture"),
            started_at,
        )),
        ReaderKind::Images => Box::new(ImagesFromResponsesSseReader::from_pump(
            pump,
            collector,
            started_at,
            ImagesResponseFormat::B64Json,
        )),
    }
}

fn read(kind: ReaderKind, pump: UpstreamSseFramePump) -> String {
    let mut body = String::new();
    reader(kind, pump)
        .read_to_string(&mut body)
        .expect("read downstream SSE");
    body
}

// Exercise an SSE client parser rather than assuming that each data line is an
// independent event. An unterminated/truncated frame could otherwise hide errors.
fn events(body: &str) -> Vec<Value> {
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            futures_util::stream::iter([Ok::<_, io::Error>(Bytes::copy_from_slice(
                body.as_bytes(),
            ))])
            .eventsource()
            .filter_map(|event| async {
                let event = event.expect("valid downstream SSE framing");
                if event.data == "[DONE]" {
                    return None;
                }
                Some(serde_json::from_str(&event.data).expect("complete downstream JSON"))
            })
            .collect()
            .await
        })
}

fn assert_budget_error(kind: ReaderKind, body: &str, code: &str) {
    let errors = events(body)
        .into_iter()
        .filter_map(|event| {
            event
                .get("error")
                .or_else(|| event.pointer("/response/error"))
                .cloned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        errors.len(),
        1,
        "{kind:?}: exactly one explicit error: {body}"
    );
    assert!(
        errors[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains(code)),
        "{kind:?}: preserve budget reason: {body}"
    );
    for marker in [
        "data: [DONE]",
        "event: response.completed",
        "event: message_stop",
        "\"finish_reason\":\"stop\"",
        "\"finish_reason\":\"tool_calls\"",
        "\"finishReason\":\"STOP\"",
    ] {
        assert!(
            !body.contains(marker),
            "{kind:?}: budget failure must not synthesize success: {body}"
        );
    }
}

fn partial(kind: ReaderKind) -> &'static str {
    match kind {
        ReaderKind::AnthropicNative | ReaderKind::ResponsesFromAnthropic =>
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"PARTIAL\"}}\n\n",
        _ => "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"PARTIAL\"}\n\n",
    }
}

#[test]
fn frame_budget_failure_reaches_every_shared_reader_without_false_success() {
    let _guard = crate::test_env_guard();
    for kind in ALL_READERS {
        let upstream = format!("{}data: {}", partial(kind), "X".repeat(2048));
        let output = read(kind, pump(&upstream, 37, 256, 16 * 1024));
        assert_budget_error(kind, &output, "upstream_sse_frame_too_large");
        assert!(
            !output.contains(&"X".repeat(128)),
            "{kind:?}: no truncated business payload"
        );
    }
}

#[test]
fn shared_budget_failure_reaches_every_shared_reader_without_false_success() {
    let _guard = crate::test_env_guard();
    for kind in ALL_READERS {
        let upstream = format!("data: {}", "X".repeat(512));
        let output = read(kind, pump(&upstream, 37, 1024, 96));
        assert_budget_error(kind, &output, "upstream_sse_buffer_exhausted");
    }
}

#[test]
fn cr_only_separators_survive_shared_reader_adapters_without_false_errors() {
    let _guard = crate::test_env_guard();
    let text = "中文🙂";
    let responses = format!(
        "event: response.output_text.delta\rdata: {}\r\revent: response.completed\rdata: {}\r\r",
        json!({ "type": "response.output_text.delta", "delta": text }),
        json!({ "type": "response.completed", "response": { "status": "completed" } }),
    );
    let anthropic = format!(
        "event: content_block_delta\rdata: {}\r\revent: message_stop\rdata: {}\r\r",
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": text } }),
        json!({ "type": "message_stop" }),
    );
    for kind in ALL_READERS
        .into_iter()
        .filter(|kind| !matches!(kind, ReaderKind::Images))
    {
        let input = match kind {
            ReaderKind::AnthropicNative | ReaderKind::ResponsesFromAnthropic => &anthropic,
            _ => &responses,
        };
        let output = read(kind, pump(input, 1, 1024, 16 * 1024));
        assert!(
            output.contains(text),
            "{kind:?}: CR-only Unicode event survives: {output}"
        );
        for event in events(&output) {
            assert!(
                event.get("error").is_none() && event.pointer("/response/error").is_none(),
                "{kind:?}: no false CR-only protocol error: {output}"
            );
        }
        if matches!(kind, ReaderKind::Responses | ReaderKind::AnthropicNative) {
            assert_eq!(output, *input, "{kind:?}: preserve all CR wire bytes");
        }
    }
}

#[test]
fn utf8_and_crlf_split_at_every_byte_survive_shared_reader_adapters() {
    let _guard = crate::test_env_guard();
    let text = "中文🙂";
    let responses = format!(
        "event: response.output_text.delta\r\ndata: {}\r\n\r\nevent: response.completed\r\ndata: {}\r\n\r\n",
        json!({ "type": "response.output_text.delta", "delta": text }),
        json!({ "type": "response.completed", "response": { "status": "completed" } }),
    );
    let anthropic = format!(
        "event: content_block_delta\r\ndata: {}\r\n\r\nevent: message_stop\r\ndata: {}\r\n\r\n",
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": text } }),
        json!({ "type": "message_stop" }),
    );
    for kind in ALL_READERS
        .into_iter()
        .filter(|kind| !matches!(kind, ReaderKind::Images))
    {
        let input = match kind {
            ReaderKind::AnthropicNative | ReaderKind::ResponsesFromAnthropic => &anthropic,
            _ => &responses,
        };
        let output = read(kind, pump(input, 1, 1024, 16 * 1024));
        assert!(
            output.contains(text),
            "{kind:?}: Unicode text survives byte boundaries: {output}"
        );
        assert!(
            !output.contains('\u{fffd}'),
            "{kind:?}: no replacement characters"
        );
        assert!(
            events(&output)
                .iter()
                .all(|event| event.get("error").is_none()),
            "{kind:?}: valid stream has no error"
        );
    }
}

#[test]
fn legal_large_image_frame_is_complete_in_responses_chat_and_images() {
    let _guard = crate::test_env_guard();
    // A legal base64 body larger than typical log previews and transport chunks.
    // The actual payload is compared, including its suffix, not only its length.
    let b64 = format!("{}aGVsbG8=", "QUJD".repeat(512 * 1024));
    assert_image_payload_survives_adapters(&b64);
}

#[test]
fn valid_png_base64_is_complete_in_responses_chat_and_images() {
    let _guard = crate::test_env_guard();
    // A complete 1x1 RGBA PNG (red pixel), including IHDR/IDAT/IEND CRCs.
    // The larger synthetic fixture above separately exercises frame size.
    let b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
    assert_image_payload_survives_adapters(b64);
}

fn assert_image_payload_survives_adapters(b64: &str) {
    let event = json!({
        "type": "response.completed",
        "response": {
            "id": "resp_image_budget", "status": "completed", "created_at": 1775900000,
            "output": [{ "type": "image_generation_call", "id": "ig_budget", "status": "completed", "output_format": "png", "result": b64 }]
        }
    });
    let upstream = format!("event: response.completed\r\ndata: {event}\r\n\r\n");
    let limit = upstream.len() + 64;
    for kind in [ReaderKind::Responses, ReaderKind::Chat, ReaderKind::Images] {
        let output = read(kind, pump(&upstream, 8191, limit, limit * 4));
        let parsed = events(&output);
        assert!(
            parsed.iter().all(|event| event.get("error").is_none()),
            "{kind:?}: legal image budget"
        );
        match kind {
            ReaderKind::Responses => {
                assert_eq!(output, upstream, "Responses preserves the full frame bytes");
                assert_eq!(parsed[0]["response"]["output"][0]["result"], b64);
            }
            ReaderKind::Chat => {
                let url = parsed
                    .iter()
                    .find_map(|event| {
                        event
                            .pointer("/choices/0/delta/images/0/image_url/url")
                            .and_then(Value::as_str)
                    })
                    .expect("chat image URL");
                assert_eq!(url, format!("data:image/png;base64,{b64}"));
            }
            ReaderKind::Images => {
                let result = parsed
                    .iter()
                    .find_map(|event| event.get("b64_json").and_then(Value::as_str))
                    .expect("images base64 payload");
                assert_eq!(result, b64);
            }
            _ => unreachable!(),
        }
    }
}
