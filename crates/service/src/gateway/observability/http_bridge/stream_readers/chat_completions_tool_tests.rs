use super::*;
use serde_json::json;

fn function_call(id: &str, name: &str, arguments: &str) -> Value {
    json!({"type": "function_call", "call_id": id, "name": name, "arguments": arguments})
}

fn item_event(kind: &str, output_index: usize, item: Value) -> Value {
    json!({"type": kind, "output_index": output_index, "item": item})
}

fn completed(output: Vec<Value>) -> Value {
    json!({"type": "response.completed", "response": {
        "id": "resp_q02", "model": "synthetic", "created_at": 1,
        "status": "completed", "output": output,
        "usage": {"input_tokens": 2, "output_tokens": 3, "total_tokens": 5}
    }})
}

fn read_events(events: &[Value]) -> String {
    let sse = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
    let pump = UpstreamSseFramePump::from_reader(Cursor::new(sse.as_bytes().to_vec()));
    read_events_with_pump(events, &sse, pump)
}

fn read_events_with_pump(events: &[Value], sse: &str, pump: UpstreamSseFramePump) -> String {
    // Compare the streamed Chat calls with the actual non-stream Responses
    // aggregation of the same upstream wire, keeping the tool payload intact.
    let (non_stream, _) =
        super::super::super::collect_non_stream_json_from_sse_bytes(sse.as_bytes());
    let non_stream: Value = serde_json::from_slice(&non_stream.unwrap()).unwrap();
    let expected = expected_tools(non_stream["output"].as_array().unwrap());
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::from_pump(
        pump,
        Arc::clone(&collector),
        Instant::now(),
    );
    let mut body = String::new();
    reader.read_to_string(&mut body).unwrap();
    assert!(body.contains("\"finish_reason\":\"tool_calls\""));
    assert!(body.contains("data: [DONE]"));
    let collector = collector.lock().unwrap();
    assert!(collector.saw_terminal);
    assert!(collector.terminal_error.is_none());
    assert_eq!(collector.usage.total_tokens, Some(5));
    assert_eq!(merged_tools(&body), expected);
    if let Ok(dir) = std::env::var("CM_REPAIR_FIXTURE_DIR") {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);
        let root = std::path::Path::new(&dir);
        assert!(root.is_absolute());
        std::fs::create_dir_all(root).unwrap();
        let name = format!("q02-{}", NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed));
        let output = events
            .last()
            .unwrap()
            .pointer("/response/output")
            .unwrap()
            .as_array()
            .unwrap();
        std::fs::write(root.join(format!("{name}.sse")), &body).unwrap();
        std::fs::write(
            root.join(format!("{name}-expected.json")),
            serde_json::to_vec_pretty(&expected_tools(output)).unwrap(),
        )
        .unwrap();
    }
    body
}

fn chunked_tool_pump(
    body: &str,
    chunk_bytes: usize,
    frame_bytes: usize,
    budget: Arc<tokio::sync::Semaphore>,
) -> UpstreamSseFramePump {
    use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem};
    let chunks = body.as_bytes().chunks(chunk_bytes).collect::<Vec<_>>();
    let (tx, rx) = tokio::sync::mpsc::channel(chunks.len() + 1);
    for chunk in chunks {
        tx.try_send(GatewayByteStreamItem::Chunk(bytes::Bytes::copy_from_slice(
            chunk,
        )))
        .expect("queue combination fixture chunk");
    }
    tx.try_send(GatewayByteStreamItem::Eof)
        .expect("queue combination fixture EOF");
    UpstreamSseFramePump::from_stream_with_limits(
        GatewayByteStream::from_receiver(rx),
        super::super::common::SseFrameLimits {
            frame_bytes,
            queued_bytes: frame_bytes * 2,
        },
        budget,
    )
}

fn assert_tool_budget_released(budget: &Arc<tokio::sync::Semaphore>, capacity: usize) {
    // Dropping the translating reader closes the pump receiver. Await that
    // asynchronous cancellation with a fixed deadline before checking permits.
    let permits = crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                Arc::clone(budget).acquire_many_owned(capacity as u32),
            )
            .await
        })
        .expect("pump releases all tool fixture storage before deadline")
        .unwrap();
    drop(permits);
    assert_eq!(budget.available_permits(), capacity);
}

// Model an append-only Chat client: a new tool must append exactly one array slot.
fn merged_tools(body: &str) -> Vec<Value> {
    let mut tools = Vec::new();
    for chunk in body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
    {
        let Some(deltas) = chunk
            .pointer("/choices/0/delta/tool_calls")
            .and_then(Value::as_array)
        else {
            continue;
        };
        for delta in deltas {
            let index = delta["index"].as_u64().unwrap() as usize;
            assert!(
                index <= tools.len(),
                "non-contiguous tool index {index}: {body}"
            );
            if index == tools.len() {
                tools.push(json!({"id": "", "type": "function", "function": {"name": "", "arguments": ""}}));
            }
            for path in ["/id", "/function/name", "/function/arguments"] {
                if let Some(part) = delta.pointer(path).and_then(Value::as_str) {
                    let target = tools[index].pointer_mut(path).unwrap();
                    *target = Value::String(format!("{}{part}", target.as_str().unwrap()));
                }
            }
        }
    }
    tools
}

fn expected_tools(output: &[Value]) -> Vec<Value> {
    output
        .iter()
        .filter(|item| item["type"] == "function_call")
        .map(|item| {
            json!({"id": item["call_id"], "type": "function", "function": {
                "name": item["name"], "arguments": item["arguments"]
            }})
        })
        .collect()
}

#[test]
fn tool_indices_start_at_zero_after_reasoning_or_message() {
    for prefix in [
        json!({"type": "reasoning", "summary": []}),
        json!({"type": "message", "content": []}),
    ] {
        let call = function_call("call_first", "first", "{\"value\":1}");
        let output = vec![prefix.clone(), call.clone()];
        let body = read_events(&[
            item_event("response.output_item.added", 0, prefix),
            item_event(
                "response.output_item.added",
                1,
                function_call("call_first", "first", ""),
            ),
            json!({"type": "response.function_call_arguments.delta", "output_index": 1, "delta": "{\"value\":"}),
            json!({"type": "response.function_call_arguments.done", "output_index": 1, "arguments": "{\"value\":1}"}),
            item_event("response.output_item.done", 1, call),
            completed(output.clone()),
        ]);
        assert_eq!(merged_tools(&body), expected_tools(&output));
    }
}

fn interleaved_fixture() -> (String, Vec<Value>) {
    let reasoning = json!({"type": "reasoning", "summary": []});
    let message = json!({"type": "message", "content": []});
    let output = vec![
        reasoning.clone(),
        function_call("call_a", "alpha", "{\"a\":1}"),
        message.clone(),
        function_call("call_b", "beta", "{\"b\":2}"),
        reasoning.clone(),
        function_call("call_c", "gamma", "{\"c\":3}"),
    ];
    let body = read_events(&[
        item_event("response.output_item.added", 0, reasoning),
        item_event(
            "response.output_item.added",
            1,
            function_call("call_a", "alpha", "{"),
        ),
        item_event("response.output_item.added", 2, message),
        item_event(
            "response.output_item.added",
            3,
            function_call("call_b", "beta", ""),
        ),
        json!({"type": "response.function_call_arguments.delta", "output_index": 3, "delta": "{\"b\":"}),
        json!({"type": "response.function_call_arguments.delta", "output_index": 1, "delta": "\"a\":"}),
        json!({"type": "response.function_call_arguments.done", "output_index": 3, "arguments": "{\"b\":2}"}),
        item_event("response.output_item.done", 3, output[3].clone()),
        completed(output.clone()),
    ]);
    (body, output)
}

#[test]
fn tool_indices_interleave_and_completed_fills_only_missing_arguments() {
    let (body, output) = interleaved_fixture();
    assert_eq!(merged_tools(&body), expected_tools(&output));
}

#[test]
fn tool_indices_done_only_and_completed_only_are_contiguous() {
    let output = vec![
        json!({"type": "reasoning"}),
        function_call("call_a", "alpha", "{}"),
        json!({"type": "message", "content": []}),
        function_call("call_b", "beta", "{\"b\":2}"),
    ];
    for events in [
        vec![completed(output.clone())],
        vec![
            item_event("response.output_item.done", 1, output[1].clone()),
            item_event("response.output_item.done", 3, output[3].clone()),
            completed(output.clone()),
        ],
    ] {
        assert_eq!(merged_tools(&read_events(&events)), expected_tools(&output));
    }
}

#[test]
fn tool_indices_recover_metadata_after_arguments_without_added() {
    let output = vec![
        json!({"type": "reasoning"}),
        function_call("call_late", "late", "{\"v\":1}"),
    ];
    for recovery in ["response.output_item.done", "response.completed"] {
        for include_delta in [false, true] {
            let mut events = vec![];
            if include_delta {
                events.push(json!({"type": "response.function_call_arguments.delta", "output_index": 1, "delta": "{\"v\":"}));
            }
            events.push(json!({"type": "response.function_call_arguments.done", "output_index": 1, "arguments": "{\"v\":1}"}));
            if recovery == "response.output_item.done" {
                events.push(item_event(recovery, 1, output[1].clone()));
            }
            events.push(completed(output.clone()));
            assert_eq!(merged_tools(&read_events(&events)), expected_tools(&output));
        }
    }
}

#[test]
fn tool_indices_recover_id_after_name_and_arguments_are_complete() {
    let output = vec![
        json!({"type": "reasoning"}),
        function_call("call_late", "late", "{}"),
    ];
    let body = read_events(&[
        item_event(
            "response.output_item.added",
            1,
            function_call("", "late", "{}"),
        ),
        completed(output.clone()),
    ]);
    assert_eq!(merged_tools(&body), expected_tools(&output));
    for chunk in body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
    {
        if let Some(tools) = chunk
            .pointer("/choices/0/delta/tool_calls")
            .and_then(Value::as_array)
        {
            assert!(tools.iter().all(|tool| tool["function"].is_object()));
        }
    }
}

#[test]
fn tool_indices_fill_missing_name_and_id_without_repeating_arguments() {
    let output = vec![
        json!({"type": "message", "content": []}),
        function_call("call_late", "late", "{\"v\":1}"),
    ];
    let body = read_events(&[
        item_event("response.output_item.added", 1, function_call("", "", "{")),
        json!({"type": "response.function_call_arguments.delta", "output_index": 1, "delta": "\"v\":"}),
        completed(output.clone()),
    ]);
    assert_eq!(merged_tools(&body), expected_tools(&output));
}

#[test]
fn tool_indices_replayed_added_and_done_snapshots_do_not_duplicate_fields() {
    let call = function_call("call_a", "alpha", "{\"a\":1}");
    let output = vec![call.clone()];
    let added = item_event(
        "response.output_item.added",
        0,
        function_call("call_a", "alpha", "{"),
    );
    let done = item_event("response.output_item.done", 0, call);
    let body = read_events(&[
        added.clone(),
        json!({"type": "response.function_call_arguments.delta", "output_index": 0, "delta": "\"a\":"}),
        added,
        done.clone(),
        done,
        completed(output.clone()),
    ]);
    assert_eq!(merged_tools(&body), expected_tools(&output));
}

#[test]
fn tool_indices_large_interleaved_recovery_survives_utf8_and_crlf_chunks() {
    let _guard = crate::test_env_guard();
    let reasoning =
        json!({"type": "reasoning", "summary": [{"type": "summary_text", "text": "先想🙂"}]});
    let message = json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "开始检查中文🙂"}]});
    // The final argument frame is larger than 64 KiB. Repeated Unicode ensures
    // the seven-byte transport chunks split inside code points as well as CRLF.
    let payload = "中文🙂".repeat(8192);
    let arguments = serde_json::to_string(&json!({"label": payload})).unwrap();
    let partial_arguments = format!("{{\"label\":\"{}", "中文🙂".repeat(4096));
    let output = vec![
        reasoning.clone(),
        message.clone(),
        function_call("call_large", "large", &arguments),
        json!({"type": "code_interpreter_call", "id": "non_function", "status": "completed"}),
        function_call("call_beta", "beta", "{\"b\":2}"),
        json!({"type": "message", "content": []}),
        function_call("call_completed", "completed_only", "{\"尾部\":\"完整🙂\"}"),
    ];
    let events = vec![
        item_event(
            "response.output_item.added",
            0,
            json!({"type": "reasoning", "summary": []}),
        ),
        json!({"type": "response.reasoning_summary_text.delta", "output_index": 0, "delta": "先想🙂"}),
        item_event(
            "response.output_item.added",
            1,
            json!({"type": "message", "role": "assistant", "content": []}),
        ),
        json!({"type": "response.output_text.delta", "output_index": 1, "delta": "开始检查中文🙂"}),
        item_event("response.output_item.added", 2, function_call("", "", "")),
        item_event("response.output_item.added", 3, output[3].clone()),
        item_event(
            "response.output_item.added",
            4,
            function_call("call_beta", "beta", ""),
        ),
        json!({"type": "response.function_call_arguments.delta", "output_index": 2, "delta": partial_arguments}),
        json!({"type": "response.function_call_arguments.delta", "output_index": 4, "delta": "{\"b\":"}),
        json!({"type": "response.function_call_arguments.done", "output_index": 4, "arguments": "{\"b\":2}"}),
        item_event("response.output_item.done", 4, output[4].clone()),
        item_event("response.output_item.done", 4, output[4].clone()),
        // done supplies the large suffix, completed supplies its missing ID/name
        // and the last tool. Replayed snapshots must not duplicate any field.
        json!({"type": "response.function_call_arguments.done", "output_index": 2, "arguments": arguments}),
        completed(output.clone()),
    ];
    let sse = events
        .iter()
        .map(|event| {
            format!(
                "event: {}\r\ndata: {event}\r\n\r\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect::<String>();
    assert!(sse.len() > 64 * 1024);
    assert!(sse
        .as_bytes()
        .chunks(7)
        .any(|chunk| std::str::from_utf8(chunk).is_err()));
    assert!(sse
        .as_bytes()
        .windows(2)
        .enumerate()
        .any(|(index, pair)| pair == b"\r\n" && (index + 1) % 7 == 0));
    let capacity = sse.len() * 4 + 4096;
    let budget = Arc::new(tokio::sync::Semaphore::new(capacity));
    let pump = chunked_tool_pump(&sse, 7, sse.len(), Arc::clone(&budget));
    let body = read_events_with_pump(&events, &sse, pump);
    assert_eq!(merged_tools(&body), expected_tools(&output));
    assert!(!body.contains('\u{fffd}'));
    for (field, expected) in [
        ("content", "开始检查中文🙂"),
        ("reasoning_content", "先想🙂"),
    ] {
        let text = body
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .filter_map(|chunk| {
                chunk["choices"][0]["delta"][field]
                    .as_str()
                    .map(str::to_owned)
            })
            .collect::<String>();
        assert_eq!(text, expected, "preserve mixed {field} exactly once");
    }
    assert_tool_budget_released(&budget, capacity);
}

#[test]
fn tool_indices_partial_output_then_budget_failure_never_finishes_successfully() {
    let _guard = crate::test_env_guard();
    let partial_tool = function_call("call_partial", "partial", "{\"v\":");
    let prefix = [
        item_event("response.output_item.added", 0, json!({"type": "reasoning"})),
        item_event("response.output_item.added", 1, function_call("call_partial", "partial", "")),
        json!({"type": "response.function_call_arguments.delta", "output_index": 1, "delta": "{\"v\":"}),
    ].iter().map(|event| format!("data: {event}\r\n\r\n")).collect::<String>();
    for (frame_bytes, capacity, code) in [
        (1024, 16 * 1024, "upstream_sse_frame_too_large"),
        (8192, 4096, "upstream_sse_buffer_exhausted"),
    ] {
        // A complete success event queued after the offending frame must never
        // overtake its explicit error or repair this partial tool into success.
        let sse = format!(
            "{prefix}data: {}\r\n\r\ndata: {}\r\n\r\n",
            "X".repeat(8192),
            completed(vec![function_call("call_partial", "partial", "{\"v\":1}")])
        );
        let budget = Arc::new(tokio::sync::Semaphore::new(capacity));
        let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
        let mut reader = ChatCompletionsFromResponsesSseReader::from_pump(
            chunked_tool_pump(&sse, 37, frame_bytes, Arc::clone(&budget)),
            Arc::clone(&collector),
            Instant::now(),
        );
        let mut body = String::new();
        reader.read_to_string(&mut body).unwrap();
        assert_eq!(merged_tools(&body), expected_tools(&[partial_tool.clone()]));
        let chunks = body
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|data| {
                serde_json::from_str::<Value>(data)
                    .expect("complete downstream JSON; no DONE on error")
            })
            .collect::<Vec<_>>();
        let errors = chunks
            .iter()
            .filter_map(|chunk| chunk.get("error"))
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1, "one explicit budget error: {body}");
        assert_eq!(errors[0]["code"], "upstream_stream_read_error");
        assert!(errors[0]["message"].as_str().unwrap().contains(code));
        assert!(chunks
            .iter()
            .all(|chunk| chunk["choices"][0]["finish_reason"].is_null()));
        assert!(!body.contains("data: [DONE]"));
        assert!(
            !body.contains(&"X".repeat(128)),
            "do not expose truncated arguments"
        );
        let collector = collector.lock().unwrap();
        assert!(!collector.saw_terminal);
        assert!(!collector.usage.completed_successfully);
        assert!(collector
            .terminal_error
            .as_deref()
            .is_some_and(|error| error.contains(code)));
        drop(collector);
        drop(reader);
        assert_tool_budget_released(&budget, capacity);
    }
}

#[test]
fn tool_indices_preserve_text_and_reasoning_against_non_stream_chat() {
    let output = vec![
        json!({"type": "reasoning", "summary": [{"type": "summary_text", "text": "Think first"}]}),
        json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Checking"}]}),
        function_call("call_a", "alpha", "{}"),
    ];
    let event = completed(output);
    let body = read_events(&[event.clone()]);
    let mapped = super::super::super::body_conversion::convert_responses_body_to_chat_completions(
        &serde_json::to_vec(&event["response"]).unwrap(),
    )
    .unwrap();
    let mapped: Value = serde_json::from_slice(&mapped).unwrap();
    for field in ["content", "reasoning", "reasoning_content"] {
        let streamed = body
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .filter_map(|chunk| {
                chunk["choices"][0]["delta"][field]
                    .as_str()
                    .map(str::to_string)
            })
            .collect::<String>();
        assert!(!streamed.is_empty());
        assert_eq!(
            streamed,
            mapped["choices"][0]["message"][field].as_str().unwrap()
        );
    }
    let expected = json!([{
        "id": "call_a", "type": "function",
        "function": {"name": "alpha", "arguments": "{}"}
    }]);
    assert_eq!(mapped["choices"][0]["message"]["tool_calls"], expected);
    assert_eq!(json!(merged_tools(&body)), expected);
    assert_eq!(mapped["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(mapped["usage"]["total_tokens"], 5);
}
