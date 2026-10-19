use super::*;
use codexmanager_core::storage::RequestLog;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::net::Shutdown;

const MODEL: &str = "gpt-5.3-codex";
const PLATFORM_KEY: &str = "pk_anthropic_stream_terminal";
const KEY_ID: &str = "gk_anthropic_stream_terminal";
const ACCOUNT_ID: &str = "acc_anthropic_stream_terminal";

struct HeldOpenUpstream {
    addr: String,
    request_rx: Receiver<(CapturedUpstreamRequest, TcpStream)>,
    disconnected_rx: Receiver<std::io::Result<usize>>,
    release_tx: mpsc::Sender<()>,
    join: thread::JoinHandle<()>,
}

// Send a complete SSE terminal event, but neither EOF nor the HTTP chunked
// terminator. The test releases this connection only after the client finishes.
fn start_held_open_upstream(frames: String) -> HeldOpenUpstream {
    let listener = bind_test_listener("anthropic held-open upstream");
    let addr = listener.local_addr().expect("upstream address").to_string();
    let (request_tx, request_rx) = mpsc::channel();
    let (disconnected_tx, disconnected_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let join = thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "gateway did not connect upstream"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept upstream: {error}"),
            }
        };
        stream
            .set_nonblocking(false)
            .expect("blocking accepted upstream socket");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("upstream read timeout");
        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        reader.read_line(&mut line).expect("upstream request line");
        let path = line
            .split_whitespace()
            .nth(1)
            .expect("request path")
            .to_string();
        let mut headers = HashMap::new();
        loop {
            line.clear();
            reader
                .read_line(&mut line)
                .expect("upstream request header");
            if line == "\r\n" {
                break;
            }
            let (name, value) = line.split_once(':').expect("request header pair");
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
        let content_length = headers
            .get("content-length")
            .expect("request content length")
            .parse::<usize>()
            .expect("numeric content length");
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body).expect("upstream request body");
        drop(reader);
        request_tx
            .send((
                CapturedUpstreamRequest {
                    path,
                    headers,
                    body,
                },
                stream.try_clone().expect("upstream cleanup socket"),
            ))
            .expect("capture upstream request");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\n",
            )
            .expect("upstream response headers");
        write!(stream, "{:x}\r\n{frames}\r\n", frames.len()).expect("upstream SSE chunk");
        stream.flush().expect("flush upstream SSE");
        let mut byte = [0];
        let _ = disconnected_tx.send(stream.read(&mut byte));
        let _ = release_rx.recv_timeout(Duration::from_secs(10));
    });
    HeldOpenUpstream {
        addr,
        request_rx,
        disconnected_rx,
        release_tx,
        join,
    }
}

fn seed_anthropic_stream_account(storage: &Storage) {
    let now = now_ts();
    seed_model_catalog_models(storage, &[MODEL]);
    storage
        .insert_account(&Account {
            id: ACCOUNT_ID.to_string(),
            label: "anthropic-stream-terminal".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_anthropic_stream_terminal".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: ACCOUNT_ID.to_string(),
            id_token: String::new(),
            access_token: "access_token_anthropic_stream_terminal".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_anthropic_stream_terminal".to_string()),
            last_refresh: now,
        })
        .expect("insert account token");
    storage
        .insert_api_key(&ApiKey {
            id: KEY_ID.to_string(),
            name: Some("anthropic-stream-terminal".to_string()),
            model_slug: Some(MODEL.to_string()),
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "account_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "anthropic_native".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(PLATFORM_KEY),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert anthropic API key");
}

fn sse_frame(value: Value) -> String {
    format!(
        "event: {}\ndata: {value}\n\n",
        value["type"].as_str().expect("event type")
    )
}

fn parse_sse_events(body: &str) -> Vec<Value> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|data| serde_json::from_str(data).expect("Anthropic SSE JSON"))
        .collect()
}

fn wait_for_request_log(storage: &Storage) -> RequestLog {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let logs = storage
            .list_request_logs(Some(&format!("key:={KEY_ID}")), 10)
            .expect("list stream request logs");
        if let Some(log) = logs.into_iter().next() {
            return log;
        }
        assert!(
            Instant::now() < deadline,
            "stream request log was not persisted"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn run_held_open_stream(case: &str, frames: String, with_tool: bool) -> (Vec<Value>, RequestLog) {
    let dir = new_test_dir(&format!("codexmanager-anthropic-stream-{case}"));
    let db_path = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    seed_anthropic_stream_account(&storage);
    let upstream = start_held_open_upstream(frames);
    let _upstream_guard = EnvGuard::set(
        "CODEXMANAGER_UPSTREAM_BASE_URL",
        &format!("http://{}/backend-api/codex", upstream.addr),
    );
    let server = codexmanager_service::start_one_shot_server().expect("start gateway");
    let gateway_addr = server.addr.clone();
    let mut request = json!({
        "model": MODEL,
        "max_tokens": 64,
        "messages": [{ "role": "user", "content": "hello" }],
        "stream": true
    });
    if with_tool {
        request["tools"] = json!([{
            "name": "search",
            "description": "Search documents",
            "input_schema": { "type": "object", "properties": {"query": {"type": "string"}} }
        }]);
    }
    let (response_tx, response_rx) = mpsc::channel();
    let client_join = thread::spawn(move || {
        let result = (|| -> Result<(u16, String, String), reqwest::Error> {
            let response = reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(8))
                .build()?
                .post(format!("http://{gateway_addr}/v1/messages?beta=true"))
                .header("x-api-key", PLATFORM_KEY)
                .header("anthropic-version", "2023-06-01")
                .json(&request)
                .send()?;
            let status = response.status().as_u16();
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            Ok((status, content_type, response.text()?))
        })();
        let _ = response_tx.send(result);
    });
    let (captured, cleanup_socket) = upstream
        .request_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("gateway reaches account upstream");
    // Completion must arrive while the upstream is deliberately still open.
    let response = response_rx.recv_timeout(Duration::from_secs(3));
    let disconnected = if matches!(&response, Ok(Ok(_))) {
        Some(
            upstream
                .disconnected_rx
                .recv_timeout(Duration::from_secs(2)),
        )
    } else {
        None
    };
    let _ = upstream.release_tx.send(());
    let _ = cleanup_socket.shutdown(Shutdown::Both);
    upstream.join.join().expect("join held-open upstream");
    server.join();
    client_join.join().expect("join HTTP client");

    let (status, content_type, body) = response
        .expect("Anthropic stream must finish before upstream EOF")
        .expect("read completed Anthropic HTTP stream");
    assert_eq!(status, 200, "gateway response: {body}");
    assert!(
        content_type.starts_with("text/event-stream"),
        "content type: {content_type}"
    );
    assert!(
        matches!(disconnected, Some(Ok(Ok(0)))),
        "gateway must release its upstream response after the terminal frame: {disconnected:?}"
    );
    assert_eq!(captured.path, "/backend-api/codex/responses");
    assert!(captured
        .headers
        .get("authorization")
        .is_some_and(|value| value.starts_with("Bearer ") && !value.contains(PLATFORM_KEY)));
    assert!(!captured.headers.contains_key("anthropic-version"));
    let upstream_payload: Value = serde_json::from_slice(&decode_upstream_request_body(&captured))
        .expect("translated Responses request");
    assert_eq!(upstream_payload["stream"], true);
    assert_eq!(upstream_payload["input"][0]["role"], "user");
    if with_tool {
        assert_eq!(upstream_payload["tools"][0]["type"], "function");
        assert_eq!(upstream_payload["tools"][0]["name"], "search");
    }
    let log = wait_for_request_log(&storage);
    assert_eq!(log.account_id.as_deref(), Some(ACCOUNT_ID));
    assert_eq!(log.original_path.as_deref(), Some("/v1/messages?beta=true"));
    assert_eq!(log.adapted_path.as_deref(), Some("/v1/responses"));
    assert_eq!(
        log.response_adapter.as_deref(),
        Some("AnthropicMessagesFromResponses")
    );
    (parse_sse_events(&body), log)
}

#[test]
fn gateway_anthropic_text_stream_finishes_at_terminal_event_without_upstream_eof() {
    let _lock = test_env_guard();
    for terminal_type in ["response.completed", "response.done"] {
        let mut frames = sse_frame(json!({"type": "response.output_text.delta", "delta": "hello"}));
        frames.push_str(&sse_frame(json!({
                "type": terminal_type,
                "response": {
                    "id": "resp_anthropic_stream_text", "model": MODEL, "status": "completed",
                    "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "hello"}]}],
                    "usage": {"input_tokens": 5, "output_tokens": 2, "total_tokens": 7}
                }
            })));
        let (events, log) = run_held_open_stream(terminal_type, frames, false);
        assert_eq!(
            events
                .iter()
                .filter(|event| event["type"] == "message_stop")
                .count(),
            1
        );
        assert_eq!(
            events.last().expect("terminal SSE event")["type"],
            "message_stop"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event["delta"]["text"] == "hello")
                .count(),
            1
        );
        let delta = events
            .iter()
            .find(|event| event["type"] == "message_delta")
            .expect("message delta");
        assert_eq!(delta["delta"]["stop_reason"], "end_turn");
        assert_eq!(delta["usage"]["output_tokens"], 2);
        assert!(!events.iter().any(|event| event["type"] == "error"));
        assert_eq!(log.status_code, Some(200));
        assert!(
            log.error.is_none(),
            "successful stream log: {:?}",
            log.error
        );
        assert_eq!(log.input_tokens, Some(5));
        assert_eq!(log.output_tokens, Some(2));
    }
}

#[test]
fn gateway_anthropic_tool_stream_finishes_at_terminal_event_without_upstream_eof() {
    let _lock = test_env_guard();
    for terminal_type in ["response.completed", "response.done"] {
        let tool = json!({"type": "function_call", "call_id": "call_search", "name": "search", "arguments": "{\"query\":\"hello\"}"});
        let mut frames =
            sse_frame(json!({"type": "response.output_item.done", "item": tool.clone()}));
        frames.push_str(&sse_frame(json!({
            "type": terminal_type,
            "response": {
                "id": "resp_anthropic_stream_tool", "model": MODEL, "status": "completed",
                "output": [tool],
                "usage": {"input_tokens": 5, "output_tokens": 3, "total_tokens": 8}
            }
        })));
        let (events, log) = run_held_open_stream(terminal_type, frames, true);
        let blocks: Vec<_> = events
            .iter()
            .filter(|event| event["type"] == "content_block_start")
            .collect();
        assert_eq!(
            blocks.len(),
            1,
            "completed snapshot must not repeat the tool call"
        );
        assert_eq!(blocks[0]["content_block"]["type"], "tool_use");
        assert_eq!(blocks[0]["content_block"]["id"], "call_search");
        assert_eq!(blocks[0]["content_block"]["name"], "search");
        let arguments: String = events
            .iter()
            .filter_map(|event| event["delta"]["partial_json"].as_str())
            .collect();
        assert_eq!(
            serde_json::from_str::<Value>(&arguments).expect("tool arguments"),
            json!({"query": "hello"})
        );
        let delta = events
            .iter()
            .find(|event| event["type"] == "message_delta")
            .expect("message delta");
        assert_eq!(delta["delta"]["stop_reason"], "tool_use");
        assert_eq!(
            events
                .iter()
                .filter(|event| event["type"] == "message_stop")
                .count(),
            1
        );
        assert_eq!(
            events.last().expect("terminal SSE event")["type"],
            "message_stop"
        );
        assert_eq!(log.status_code, Some(200));
        assert!(
            log.error.is_none(),
            "successful tool stream log: {:?}",
            log.error
        );
    }
}

#[test]
fn gateway_anthropic_failed_stream_emits_error_and_records_failure() {
    let _lock = test_env_guard();
    for terminal_type in ["response.failed", "response.incomplete", "error"] {
        let error = json!({"type": "server_error", "code": "upstream_failed", "message": "upstream refused this turn"});
        let terminal = if terminal_type == "error" {
            json!({"type": terminal_type, "error": error})
        } else {
            json!({"type": terminal_type, "response": {"id": "resp_anthropic_failed", "model": MODEL, "error": error}})
        };
        let mut frames =
            sse_frame(json!({"type": "response.output_text.delta", "delta": "partial"}));
        frames.push_str(&sse_frame(terminal));
        let (events, log) = run_held_open_stream(terminal_type, frames, false);
        let errors: Vec<_> = events
            .iter()
            .filter(|event| event["type"] == "error")
            .collect();
        assert_eq!(
            errors.len(),
            1,
            "one Anthropic error event for {terminal_type}"
        );
        assert_eq!(errors[0]["error"]["message"], "upstream refused this turn");
        assert!(!events.iter().any(|event| event["type"] == "message_stop"));
        assert!(!events
            .iter()
            .any(|event| event["delta"]["stop_reason"] == "end_turn"));
        assert_eq!(events.last().expect("terminal SSE event")["type"], "error");
        assert_eq!(log.status_code, Some(502));
        assert!(
            log.error
                .as_deref()
                .is_some_and(|error| error.contains("upstream refused this turn")),
            "failure log error: {:?}",
            log.error
        );
    }
}
