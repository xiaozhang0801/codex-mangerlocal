use super::*;
use bytes::Bytes;

const WAIT: Duration = Duration::from_secs(3);

fn limits(frame_bytes: usize, queued_bytes: usize) -> SseFrameLimits {
    SseFrameLimits {
        frame_bytes,
        queued_bytes,
    }
}

fn input() -> (
    mpsc::Sender<GatewayByteStreamItem>,
    GatewayByteStream,
    tokio::sync::oneshot::Receiver<()>,
) {
    let (tx, rx) = mpsc::channel(256);
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    (
        tx,
        GatewayByteStream::from_receiver_with_cancel(rx, Some(cancel)),
        cancelled,
    )
}

fn chunk(tx: &mpsc::Sender<GatewayByteStreamItem>, bytes: &[u8]) {
    tx.try_send(GatewayByteStreamItem::Chunk(Bytes::copy_from_slice(bytes)))
        .expect("queue upstream fixture chunk");
}

async fn until(mut ready: impl FnMut() -> bool) {
    tokio::time::timeout(WAIT, async {
        while !ready() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("pump reached expected resource state");
}

async fn recv(pump: &mut UpstreamSseFramePump) -> UpstreamSseFramePumpItem {
    tokio::time::timeout(WAIT, pump.recv_async())
        .await
        .expect("pump cannot deadlock")
        .expect("pump item")
}

fn storage_cost(bytes: usize, lines: usize) -> usize {
    2 * bytes + LINE_STORAGE_BYTES * lines
}

#[test]
fn long_line_across_chunks_and_many_lines_without_blank_frame_obey_frame_limit() {
    for bytes in [vec![b'x'; 4096], b"data: x\n".repeat(512)] {
        let budget = Arc::new(Semaphore::new(64 * 1024));
        let mut frame = FrameBuffer::default();
        let mut failure = None;
        'chunks: for part in bytes.chunks(17) {
            let mut input = part;
            while !input.is_empty() {
                match frame.push(input, 1024, &budget) {
                    Ok((rest, complete)) => {
                        assert!(!complete, "fixture has no blank frame separator");
                        input = rest;
                    }
                    Err(error) => {
                        failure = Some(error);
                        break 'chunks;
                    }
                }
            }
        }
        assert!(failure
            .unwrap()
            .starts_with("upstream_sse_frame_too_large:"));
        assert!(frame.bytes <= 1024);
        assert!(!frame.lines.iter().any(|line| line == "\n"));
        drop(frame);
        assert_eq!(budget.available_permits(), 64 * 1024);
    }
}

#[test]
fn huge_chunk_is_rejected_after_scanning_only_the_remaining_frame_budget() {
    const LIMIT: usize = 1024;
    let mut huge = vec![b'x'; 1024 * 1024];
    *huge.last_mut().unwrap() = b'\n';
    for initial in [b"".as_slice(), b"data: ".as_slice()] {
        let capacity = 4096;
        let budget = Arc::new(Semaphore::new(capacity));
        let mut frame = FrameBuffer::default();
        if !initial.is_empty() {
            frame.push(initial, LIMIT, &budget).unwrap();
        }
        let charged = capacity - budget.available_permits();
        let error = frame.push(&huge, LIMIT, &budget).unwrap_err();
        assert!(error.starts_with("upstream_sse_frame_too_large:"));
        assert_eq!(frame.scanned_bytes, LIMIT + 1);
        assert_eq!(frame.bytes, initial.len());
        assert_eq!(frame.line, initial);
        assert_eq!(budget.available_permits(), capacity - charged);
        drop(frame);
        assert_eq!(budget.available_permits(), capacity);
    }
}

#[test]
fn parser_scans_each_byte_once_and_charges_payload_and_line_storage() {
    let long = format!("data: {}\r\n\r\n", "x".repeat(16 * 1024));
    let short = format!("{}\n", "x\n".repeat(4096));
    for (body, chunk_bytes) in [(long, 1), (short, 113)] {
        let line_count = body.bytes().filter(|byte| *byte == b'\n').count();
        let cost = storage_cost(body.len(), line_count);
        let budget = Arc::new(Semaphore::new(cost + 1));
        let mut frame = FrameBuffer::default();
        let mut completed = 0;
        for part in body.as_bytes().chunks(chunk_bytes) {
            let mut input = part;
            while !input.is_empty() {
                let (rest, complete) = frame.push(input, body.len(), &budget).unwrap();
                input = rest;
                completed += usize::from(complete);
            }
        }
        assert_eq!(completed, 1);
        assert_eq!(frame.scanned_bytes, body.len(), "no prefix rescanning");
        assert_eq!(frame.lines.concat(), body);
        assert_eq!(
            budget.available_permits(),
            1,
            "charge 2 * bytes plus per-line overhead"
        );
        let (lines, bytes, permit) = frame.take();
        assert_eq!(bytes, body.len());
        assert_eq!(lines.concat(), body);
        assert_eq!(permit.as_ref().unwrap().num_permits() as usize, cost);
        drop(permit);
        assert_eq!(budget.available_permits(), cost + 1);
    }
}

#[test]
fn line_descriptor_limit_bounds_many_tiny_lines_before_payload_budget() {
    let capacity = storage_cost(2 * MAX_FRAME_LINES, MAX_FRAME_LINES) + 1024;
    let budget = Arc::new(Semaphore::new(capacity));
    let mut frame = FrameBuffer::default();
    for _ in 0..MAX_FRAME_LINES {
        let (rest, complete) = frame.push(b"x\n", 4 * MAX_FRAME_LINES, &budget).unwrap();
        assert!(rest.is_empty());
        assert!(!complete);
    }
    assert_eq!(frame.lines.len(), MAX_FRAME_LINES);
    let error = frame.push(b"\n", 4 * MAX_FRAME_LINES, &budget).unwrap_err();
    assert!(error.contains("upstream_sse_frame_too_large"));
    assert!(error.contains("lines"));
    assert_eq!(frame.lines.len(), MAX_FRAME_LINES);
    drop(frame);
    assert_eq!(budget.available_permits(), capacity);
}

#[test]
fn eof_tail_and_split_unicode_crlf_preserve_all_bytes_and_release_storage() {
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let body = "event: fixture\r\ndata: {\"text\":\"中文🙂\"}";
            let (tx, source, cancelled) = input();
            for byte in body.bytes() {
                chunk(&tx, &[byte]);
            }
            tx.try_send(GatewayByteStreamItem::Eof).unwrap();
            let budget = Arc::new(Semaphore::new(8192));
            let mut pump = UpstreamSseFramePump::from_stream_with_limits(
                source,
                limits(1024, 2048),
                budget.clone(),
            );
            match recv(&mut pump).await {
                UpstreamSseFramePumpItem::Frame(frame) => {
                    assert_eq!(frame, ["event: fixture\r\n", "data: {\"text\":\"中文🙂\"}"]);
                    assert_eq!(
                        budget.available_permits(),
                        8192 - storage_cost(body.len(), 2)
                    );
                }
                other => panic!("expected complete EOF tail, got {other:?}"),
            }
            assert!(matches!(
                recv(&mut pump).await,
                UpstreamSseFramePumpItem::Eof
            ));
            tokio::time::timeout(WAIT, cancelled)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(budget.available_permits(), 8192);
            // Completion acknowledgement must also work after the producer reached EOF.
            tokio::time::timeout(WAIT, pump.stop_input())
                .await
                .expect("EOF stop acknowledgement");
        });
}

#[test]
fn invalid_utf8_at_newline_or_eof_is_an_error_and_frees_partial_frame() {
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            for tail in [
                b"data: \xf0\x9f\n\n".as_slice(),
                b"data: \xf0\x9f".as_slice(),
            ] {
                let (tx, source, cancelled) = input();
                for byte in tail {
                    chunk(&tx, &[*byte]);
                }
                tx.try_send(GatewayByteStreamItem::Eof).unwrap();
                let budget = Arc::new(Semaphore::new(4096));
                let mut pump = UpstreamSseFramePump::from_stream_with_limits(
                    source,
                    limits(1024, 2048),
                    budget.clone(),
                );
                match recv(&mut pump).await {
                    UpstreamSseFramePumpItem::Error(error) => {
                        assert!(error.contains("upstream_sse_invalid_utf8"))
                    }
                    other => panic!("invalid UTF-8 cannot be silently replaced: {other:?}"),
                }
                tokio::time::timeout(WAIT, cancelled)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(budget.available_permits(), 4096);
            }
        });
}

#[test]
fn slow_consumer_hits_byte_backpressure_before_frame_count_and_drop_cancels() {
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let frame = b"data: 0123456789\n\n";
            let cost = storage_cost(frame.len(), 2);
            let (tx, source, cancelled) = input();
            chunk(&tx, &frame.repeat(10));
            let capacity = 8192;
            let budget = Arc::new(Semaphore::new(capacity));
            let pump = UpstreamSseFramePump::from_stream_with_limits(
                source,
                limits(frame.len(), frame.len()),
                budget.clone(),
            );
            until(|| pump.rx.len() == 1 && budget.available_permits() == capacity - 2 * cost).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert_eq!(
                pump.rx.len(),
                1,
                "byte queue fills while the 128-frame channel has room"
            );
            assert_eq!(
                budget.available_permits(),
                capacity - 2 * cost,
                "only one queued and one waiting frame are stored"
            );
            drop(pump);
            tokio::time::timeout(WAIT, cancelled)
                .await
                .unwrap()
                .unwrap();
            until(|| budget.available_permits() == capacity).await;
            assert!(tx.is_closed());
        });
}

#[test]
fn frame_overflow_cancels_upstream_before_error_is_consumed_and_frees_partial_bytes() {
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let valid = b"data: valid\n\n";
            let (tx, source, cancelled) = input();
            chunk(&tx, valid);
            chunk(&tx, &[b'x'; 64]);
            chunk(&tx, &[b'x'; 64]);
            let capacity = 4096;
            let budget = Arc::new(Semaphore::new(capacity));
            let mut pump = UpstreamSseFramePump::from_stream_with_limits(
                source,
                limits(100, 100),
                budget.clone(),
            );
            tokio::time::timeout(WAIT, cancelled)
                .await
                .unwrap()
                .unwrap();
            until(|| pump.rx.len() == 2).await;
            assert_eq!(pump.rx.len(), 2, "valid frame followed by explicit error");
            assert_eq!(
                budget.available_permits(),
                capacity - storage_cost(valid.len(), 2),
                "failed partial frame released before client reads"
            );
            match recv(&mut pump).await {
                UpstreamSseFramePumpItem::Frame(lines) => {
                    assert_eq!(lines.concat().as_bytes(), valid)
                }
                other => panic!("retain previously completed frame: {other:?}"),
            }
            assert!(
                budget.available_permits() < capacity,
                "active frame remains charged until next receive"
            );
            match recv(&mut pump).await {
                UpstreamSseFramePumpItem::Error(error) => {
                    assert!(error.contains("upstream_sse_frame_too_large"))
                }
                other => panic!("expected byte budget error: {other:?}"),
            }
            assert_eq!(budget.available_permits(), capacity);
            assert!(tx.is_closed());
        });
}

#[test]
fn incomplete_long_frame_updates_network_activity_before_any_frame_is_ready() {
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let (tx, source, cancelled) = input();
            let capacity = 32 * 1024;
            let budget = Arc::new(Semaphore::new(capacity));
            let mut pump = UpstreamSseFramePump::from_stream_with_limits(
                source,
                limits(8192, 8192),
                budget.clone(),
            );
            let start = pump.last_activity();
            tokio::time::sleep(Duration::from_millis(5)).await;
            chunk(&tx, b"data: ");
            until(|| pump.last_activity() > start).await;
            let first_byte_activity = pump.last_activity();
            assert_eq!(
                pump.rx.len(),
                0,
                "partial long line is not a completed frame"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
            chunk(&tx, &[b'x'; 4096]);
            until(|| {
                pump.last_activity() > first_byte_activity
                    && budget.available_permits() == capacity - storage_cost(4102, 1)
            })
            .await;
            assert_eq!(
                pump.rx.len(),
                0,
                "large frame remains unfinished while bytes arrive"
            );
            assert_eq!(budget.available_permits(), capacity - storage_cost(4102, 1));
            chunk(&tx, b"\r\n\r\n");
            tx.try_send(GatewayByteStreamItem::Eof).unwrap();
            match recv(&mut pump).await {
                UpstreamSseFramePumpItem::Frame(lines) => assert_eq!(
                    lines.concat(),
                    format!("data: {}\r\n\r\n", "x".repeat(4096))
                ),
                other => panic!("continued long frame completes intact: {other:?}"),
            }
            assert!(matches!(
                recv(&mut pump).await,
                UpstreamSseFramePumpItem::Eof
            ));
            tokio::time::timeout(WAIT, cancelled)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(budget.available_permits(), capacity);
        });
}

#[test]
fn concurrent_pumps_share_a_hard_budget_and_fail_without_waiting_for_each_other() {
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let capacity = 160;
            let budget = Arc::new(Semaphore::new(capacity));
            let (left_tx, left_source, left_cancel) = input();
            let (right_tx, right_source, right_cancel) = input();
            let mut left = UpstreamSseFramePump::from_stream_with_limits(
                left_source,
                limits(1024, 1024),
                budget.clone(),
            );
            let mut right = UpstreamSseFramePump::from_stream_with_limits(
                right_source,
                limits(1024, 1024),
                budget.clone(),
            );
            chunk(&left_tx, &[b'x'; 32]);
            chunk(&right_tx, &[b'x'; 32]);
            until(|| left_tx.is_closed() != right_tx.is_closed()).await;
            assert_eq!(
                budget.available_permits(),
                capacity - storage_cost(32, 1),
                "only one partial frame fits"
            );
            let winner = if left_tx.is_closed() {
                &right_tx
            } else {
                &left_tx
            };
            chunk(winner, &[b'x'; 64]);
            for pump in [&mut left, &mut right] {
                match recv(pump).await {
                    UpstreamSseFramePumpItem::Error(error) => {
                        assert!(error.contains("upstream_sse_buffer_exhausted"))
                    }
                    other => panic!("global admission failure must be explicit: {other:?}"),
                }
            }
            tokio::time::timeout(WAIT, left_cancel)
                .await
                .unwrap()
                .unwrap();
            tokio::time::timeout(WAIT, right_cancel)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(budget.available_permits(), capacity);
        });
}

#[test]
fn stop_input_acknowledges_byte_backpressure_then_drains_complete_frames() {
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let frames = ["data: 0001\n\n", "data: 0002\n\n", "data: 0003\n\n"];
            let (tx, source, cancelled) = input();
            chunk(&tx, frames.concat().as_bytes());
            let budget = Arc::new(Semaphore::new(8192));
            let mut pump = UpstreamSseFramePump::from_stream_with_limits(
                source,
                limits(frames[0].len(), frames[0].len()),
                budget.clone(),
            );
            until(|| {
                pump.rx.len() == 1
                    && budget.available_permits() == 8192 - 2 * storage_cost(frames[0].len(), 2)
            })
            .await;
            tokio::time::timeout(WAIT, pump.stop_input())
                .await
                .expect("stop cannot wait for queue draining");
            tokio::time::timeout(WAIT, cancelled)
                .await
                .unwrap()
                .unwrap();
            for expected in frames {
                match recv(&mut pump).await {
                    UpstreamSseFramePumpItem::Frame(lines) => assert_eq!(lines.concat(), expected),
                    other => panic!("complete buffered frame survives stop: {other:?}"),
                }
            }
            assert!(matches!(
                recv(&mut pump).await,
                UpstreamSseFramePumpItem::Eof
            ));
            assert_eq!(budget.available_permits(), 8192);
            assert!(tx.is_closed());
        });
}

#[test]
fn stop_input_acknowledges_full_frame_channel_and_preserves_buffered_order() {
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let frames = (0..FRAME_CHANNEL_CAPACITY + 2)
                .map(|index| format!("data: {index:04}\n\n"))
                .collect::<Vec<_>>();
            let (tx, source, cancelled) = input();
            chunk(&tx, frames.concat().as_bytes());
            let capacity = 64 * 1024;
            let budget = Arc::new(Semaphore::new(capacity));
            let mut pump = UpstreamSseFramePump::from_stream_with_limits(
                source,
                limits(64, 16 * 1024),
                budget.clone(),
            );
            until(|| pump.rx.len() == FRAME_CHANNEL_CAPACITY).await;
            tokio::time::timeout(WAIT, pump.stop_input())
                .await
                .expect("full channel cannot block stop acknowledgement");
            tokio::time::timeout(WAIT, cancelled)
                .await
                .unwrap()
                .unwrap();
            for expected in frames {
                match recv(&mut pump).await {
                    UpstreamSseFramePumpItem::Frame(lines) => assert_eq!(lines.concat(), expected),
                    other => panic!("buffered order survives cancellation: {other:?}"),
                }
            }
            assert!(matches!(
                recv(&mut pump).await,
                UpstreamSseFramePumpItem::Eof
            ));
            assert_eq!(budget.available_permits(), capacity);
            assert!(tx.is_closed());
        });
}

#[test]
fn cr_lf_and_split_crlf_are_linear_and_preserve_every_wire_byte() {
    for body in [
        "event: fixture\rdata: {\"text\":\"中文🙂\"}\r\r",
        "event: fixture\r\ndata: {\"text\":\"中文🙂\"}\r\n\r\n",
        "event: fixture\rdata: {\"text\":\"中文🙂\"}\n\r\n",
    ] {
        for chunk_size in [1, 2, 3, 17, body.len()] {
            let capacity = 16 * 1024;
            let budget = Arc::new(Semaphore::new(capacity));
            let mut frame = FrameBuffer::default();
            let mut completed = Vec::new();
            for part in body.as_bytes().chunks(chunk_size) {
                let mut input = part;
                while !input.is_empty() {
                    let (rest, complete) = frame.push(input, 1024, &budget).unwrap();
                    input = rest;
                    if complete {
                        let (lines, _, storage) = frame.take();
                        completed.push(lines.concat());
                        drop(storage);
                    }
                }
            }
            assert_eq!(frame.bytes, 0, "frame must finish before EOF");
            assert_eq!(completed.concat(), body, "chunk size {chunk_size}");
            assert_eq!(frame.scanned_bytes, body.len(), "scan each byte once");
            assert_eq!(budget.available_permits(), capacity);
        }
    }
}

#[test]
fn cr_only_empty_line_delivers_on_a_live_connection_and_keeps_later_lf() {
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let first = b"event: fixture\rdata: {\"text\":\"live\"}\r\r";
            let (tx, source, cancelled) = input();
            chunk(&tx, first);
            let capacity = 8192;
            let budget = Arc::new(Semaphore::new(capacity));
            let mut pump = UpstreamSseFramePump::from_stream_with_limits(
                source,
                limits(1024, 2048),
                budget.clone(),
            );
            let mut wire = match recv(&mut pump).await {
                UpstreamSseFramePumpItem::Frame(lines) => {
                    assert_eq!(
                        lines,
                        ["event: fixture\r", "data: {\"text\":\"live\"}\r", "\r"]
                    );
                    lines.concat()
                }
                other => panic!("CR-only frame must not await EOF/next chunk: {other:?}"),
            };
            assert!(!tx.is_closed(), "provider is still connected");
            // The LF may arrive after the CR frame was already delivered. It
            // has no data semantics but must remain in native raw output.
            chunk(&tx, b"\n");
            match recv(&mut pump).await {
                UpstreamSseFramePumpItem::Frame(lines) => {
                    assert_eq!(lines, ["\n"]);
                    wire.push_str(&lines.concat());
                }
                other => panic!("retain split CRLF tail: {other:?}"),
            }
            assert_eq!(wire.as_bytes(), [first.as_slice(), b"\n"].concat());
            drop(tx);
            assert!(matches!(
                recv(&mut pump).await,
                UpstreamSseFramePumpItem::Eof
            ));
            tokio::time::timeout(WAIT, cancelled)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(budget.available_permits(), capacity);
        });
}

#[test]
fn tiny_line_allocations_fit_the_accounted_storage_including_initial_descriptors() {
    let budget = Arc::new(Semaphore::new(16 * 1024));
    let mut frame = FrameBuffer::default();
    for _ in 0..17 {
        frame.push(b"x\n", 1024, &budget).unwrap();
        let allocated = frame.lines.capacity() * std::mem::size_of::<String>()
            + frame.lines.iter().map(String::capacity).sum::<usize>()
            + frame.line.capacity();
        let charged = frame.storage.as_ref().unwrap().num_permits() as usize;
        assert!(
            allocated <= charged,
            "allocated {allocated} exceeds charge {charged}"
        );
    }
}
