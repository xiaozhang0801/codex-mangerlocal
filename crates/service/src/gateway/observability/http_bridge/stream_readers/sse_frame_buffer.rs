//! Bounded SSE framing shared by every translating reader.
use super::{Arc, GatewayByteStream, GatewayByteStreamItem};
use crate::gateway::upstream::attempt_flow::transport::runtime::upstream_runtime;
use std::sync::{mpsc::RecvTimeoutError, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};

const FRAME_CHANNEL_CAPACITY: usize = 128;
// Image results are base64, and completed events may contain several results.
// A 128 MiB frame leaves room for roughly 96 MiB of binary image data before
// base64 and JSON overhead. This is an engineering ceiling, not an upstream
// image size guarantee. It is independent of log previews.
const MAX_FRAME_BYTES: usize = 128 * 1024 * 1024;
const MAX_QUEUED_BYTES: usize = 128 * 1024 * 1024;
const MAX_SHARED_BUFFER_BYTES: usize = 512 * 1024 * 1024;
const MAX_FRAME_LINES: usize = 65_536;
// Account conservatively for geometric Vec growth, String descriptors and the
// minimum allocation for very short lines. This bounds parser/queue storage,
// not the HTTP transport, protocol JSON trees, or downstream response buffers.
const LINE_STORAGE_BYTES: usize = 2 * std::mem::size_of::<String>() + 8;

#[derive(Clone, Copy)]
pub(crate) struct SseFrameLimits {
    pub(crate) frame_bytes: usize,
    pub(crate) queued_bytes: usize,
}

impl Default for SseFrameLimits {
    fn default() -> Self {
        Self {
            frame_bytes: MAX_FRAME_BYTES,
            queued_bytes: MAX_QUEUED_BYTES,
        }
    }
}

fn shared_budget() -> Arc<Semaphore> {
    static BUDGET: OnceLock<Arc<Semaphore>> = OnceLock::new();
    BUDGET
        .get_or_init(|| Arc::new(Semaphore::new(MAX_SHARED_BUFFER_BYTES)))
        .clone()
}

#[derive(Debug)]
pub(crate) enum UpstreamSseFramePumpItem {
    Frame(Vec<String>),
    Eof,
    Error(String),
}

struct QueuedItem {
    item: UpstreamSseFramePumpItem,
    storage: Option<OwnedSemaphorePermit>,
    _queued: Option<OwnedSemaphorePermit>,
}

impl QueuedItem {
    fn terminal(item: UpstreamSseFramePumpItem) -> Self {
        Self {
            item,
            storage: None,
            _queued: None,
        }
    }
}

#[derive(Default)]
struct FrameBuffer {
    line: Vec<u8>,
    lines: Vec<String>,
    bytes: usize,
    storage: Option<OwnedSemaphorePermit>,
    // CR terminates a line immediately. A following LF belongs to that same
    // separator, including when the two bytes arrive in separate chunks.
    after_cr: bool,
    #[cfg(test)]
    scanned_bytes: usize,
}

impl FrameBuffer {
    // Only the new slice is searched. An unfinished line is never rescanned,
    // and completing a line moves its allocation instead of draining a prefix.
    fn push<'a>(
        &mut self,
        input: &'a [u8],
        limit: usize,
        budget: &Arc<Semaphore>,
    ) -> Result<(&'a [u8], bool), String> {
        if self.after_cr {
            self.after_cr = false;
            if input.first() == Some(&b'\n') && !self.lines.is_empty() {
                self.reserve_bytes(1, false, limit, budget)?;
                // The previous CR line has not left the parser yet. Preserve
                // its CRLF bytes without treating LF as a second line ending.
                self.lines.last_mut().unwrap().push('\n');
                #[cfg(test)]
                {
                    self.scanned_bytes += 1;
                }
                return Ok((&input[1..], false));
            }
            // An empty CR line may already have been delivered. In that case
            // retain the optional LF as an empty raw frame; adapters ignore its
            // absent data and native passthrough retains byte-for-byte output.
        }
        // Inspect only enough bytes to complete or reject the current frame.
        // A single oversized transport chunk must not extend the scan beyond
        // the frame's remaining budget before it can be rejected.
        let prefix = &input[..input.len().min(limit - self.bytes + 1)];
        let newline = prefix
            .iter()
            .position(|byte| *byte == b'\r' || *byte == b'\n');
        let len = newline.map_or(prefix.len(), |end| {
            end + 1 + usize::from(prefix[end] == b'\r' && prefix.get(end + 1) == Some(&b'\n'))
        });
        #[cfg(test)]
        {
            self.scanned_bytes += len;
        }
        self.reserve_bytes(len, self.line.is_empty(), limit, budget)?;
        self.line.extend_from_slice(&input[..len]);
        let mut complete = false;
        if let Some(end) = newline {
            self.after_cr = input[end] == b'\r' && len == end + 1;
            complete = self.line == b"\n" || self.line == b"\r" || self.line == b"\r\n";
            self.finish_line()?;
        }
        Ok((&input[len..], complete))
    }

    fn reserve_bytes(
        &mut self,
        len: usize,
        new_line: bool,
        limit: usize,
        budget: &Arc<Semaphore>,
    ) -> Result<(), String> {
        if self.bytes.saturating_add(len) > limit {
            return Err(format!(
                "upstream_sse_frame_too_large: SSE frame exceeds {limit} bytes"
            ));
        }
        if new_line && self.lines.len() >= MAX_FRAME_LINES {
            return Err(format!(
                "upstream_sse_frame_too_large: SSE frame exceeds {MAX_FRAME_LINES} lines"
            ));
        }
        let cost = len * 2 + if new_line { LINE_STORAGE_BYTES } else { 0 };
        let permit = budget
            .clone()
            .try_acquire_many_owned(cost as u32)
            .map_err(|_| {
                "upstream_sse_buffer_exhausted: shared SSE buffer byte budget exhausted".to_string()
            })?;
        if let Some(storage) = &mut self.storage {
            storage.merge(permit);
        } else {
            self.storage = Some(permit);
        }
        self.bytes += len;
        Ok(())
    }

    fn finish_line(&mut self) -> Result<(), String> {
        let line = String::from_utf8(std::mem::take(&mut self.line))
            .map_err(|error| format!("upstream_sse_invalid_utf8: {error}"))?;
        // Vec's first amortized allocation otherwise reserves four String
        // descriptors for one tiny line, exceeding the per-line accounting.
        if self.lines.capacity() == 0 {
            self.lines.reserve_exact(1);
        }
        self.lines.push(line);
        Ok(())
    }

    fn take(&mut self) -> (Vec<String>, usize, Option<OwnedSemaphorePermit>) {
        let bytes = std::mem::take(&mut self.bytes);
        (std::mem::take(&mut self.lines), bytes, self.storage.take())
    }
}

pub(crate) struct UpstreamSseFramePump {
    rx: mpsc::Receiver<QueuedItem>,
    // Keep the last frame charged while the reader maps/delivers it. The next
    // receive is requested only after that reader has consumed its output.
    active_storage: Option<OwnedSemaphorePermit>,
    last_activity: Option<Arc<std::sync::Mutex<Instant>>>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    stopped: Option<tokio::sync::oneshot::Receiver<()>>,
}

struct InputControl {
    stop: tokio::sync::oneshot::Receiver<()>,
    acknowledged: Option<tokio::sync::oneshot::Sender<()>>,
    stopped: bool,
}

impl InputControl {
    fn stop(&mut self, upstream: &mut GatewayByteStream) {
        upstream.close_input();
        self.stopped = true;
        if let Some(acknowledged) = self.acknowledged.take() {
            let _ = acknowledged.send(());
        }
    }
}

impl UpstreamSseFramePump {
    #[cfg(test)]
    pub(crate) fn from_receiver(mut input: mpsc::Receiver<UpstreamSseFramePumpItem>) -> Self {
        let (tx, rx) = mpsc::channel(FRAME_CHANNEL_CAPACITY);
        upstream_runtime().unwrap().spawn(async move {
            loop {
                let item = tokio::select! {
                    _ = tx.closed() => return,
                    item = input.recv() => match item { Some(item) => item, None => return },
                };
                if tx.send(QueuedItem::terminal(item)).await.is_err() {
                    return;
                }
            }
        });
        Self {
            rx,
            active_storage: None,
            last_activity: None,
            stop: None,
            stopped: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_reader<R: std::io::Read + Send + 'static>(mut upstream: R) -> Self {
        let (tx, rx) = mpsc::channel(FRAME_CHANNEL_CAPACITY);
        std::thread::spawn(move || loop {
            let mut bytes = vec![0; 8192];
            let item = match upstream.read(&mut bytes) {
                Ok(0) => GatewayByteStreamItem::Eof,
                Ok(len) => {
                    bytes.truncate(len);
                    GatewayByteStreamItem::Chunk(bytes.into())
                }
                Err(error) => GatewayByteStreamItem::Error(error.to_string()),
            };
            let terminal = !matches!(item, GatewayByteStreamItem::Chunk(_));
            if tx.blocking_send(item).is_err() || terminal {
                return;
            }
        });
        Self::from_stream(GatewayByteStream::from_receiver(rx))
    }

    #[cfg(test)]
    pub(crate) fn new(upstream: reqwest::blocking::Response) -> Self {
        Self::from_reader(upstream)
    }

    pub(crate) fn from_stream(upstream: GatewayByteStream) -> Self {
        Self::from_stream_with_limits(upstream, SseFrameLimits::default(), shared_budget())
    }

    pub(crate) fn from_stream_with_limits(
        mut upstream: GatewayByteStream,
        limits: SseFrameLimits,
        budget: Arc<Semaphore>,
    ) -> Self {
        assert!(limits.frame_bytes > 0 && limits.frame_bytes <= limits.queued_bytes);
        assert!(limits.queued_bytes <= u32::MAX as usize);
        let (tx, rx) = mpsc::channel(FRAME_CHANNEL_CAPACITY);
        let queued = Arc::new(Semaphore::new(limits.queued_bytes));
        let last_activity = Arc::new(std::sync::Mutex::new(Instant::now()));
        let activity = last_activity.clone();
        let (stop, stop_rx) = tokio::sync::oneshot::channel();
        let (acknowledged, stopped) = tokio::sync::oneshot::channel();
        if let Ok(runtime) = upstream_runtime() {
            runtime.spawn(async move {
                let mut frame = FrameBuffer::default();
                let mut control = InputControl {
                    stop: stop_rx,
                    acknowledged: Some(acknowledged),
                    stopped: false,
                };
                let result: Result<(), String> = async {
                    loop {
                        let next = tokio::select! {
                            biased;
                            _ = tx.closed() => return Ok(()),
                            _ = &mut control.stop, if !control.stopped => {
                                control.stop(&mut upstream);
                                continue;
                            }
                            next = upstream.recv_async() => next,
                        };
                        match next {
                            Some(GatewayByteStreamItem::Chunk(bytes)) => {
                                if let Ok(mut activity) = activity.lock() {
                                    *activity = Instant::now();
                                }
                                let mut input = bytes.as_ref();
                                while !input.is_empty() {
                                    if tx.is_closed() {
                                        return Ok(());
                                    }
                                    let (rest, complete) =
                                        frame.push(input, limits.frame_bytes, &budget)?;
                                    input = rest;
                                    if complete
                                        && !send_frame(
                                            &tx,
                                            &queued,
                                            &mut frame,
                                            &mut upstream,
                                            &mut control,
                                        )
                                        .await
                                    {
                                        return Ok(());
                                    }
                                }
                            }
                            Some(GatewayByteStreamItem::Error(error)) => return Err(error),
                            Some(GatewayByteStreamItem::Eof) | None => {
                                if !frame.line.is_empty() {
                                    frame.finish_line()?;
                                }
                                if frame.bytes > 0
                                    && !send_frame(
                                        &tx,
                                        &queued,
                                        &mut frame,
                                        &mut upstream,
                                        &mut control,
                                    )
                                    .await
                                {
                                    return Ok(());
                                }
                                return Ok(());
                            }
                        }
                    }
                }
                .await;
                // Cancel the network and release partial storage BEFORE waiting
                // to deliver an error to a slow consumer of an already full queue.
                upstream.close();
                control.stop(&mut upstream);
                drop(upstream);
                drop(frame);
                let terminal = match result {
                    Ok(()) => UpstreamSseFramePumpItem::Eof,
                    Err(error) => UpstreamSseFramePumpItem::Error(error),
                };
                let _ = tx.send(QueuedItem::terminal(terminal)).await;
            });
        }
        Self {
            rx,
            active_storage: None,
            last_activity: Some(last_activity),
            stop: Some(stop),
            stopped: Some(stopped),
        }
    }

    pub(crate) fn last_activity(&self) -> Option<Instant> {
        self.last_activity
            .as_ref()
            .map(|activity| *activity.lock().unwrap_or_else(|error| error.into_inner()))
    }

    pub(crate) async fn stop_input(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(stopped) = self.stopped.take() {
            let _ = stopped.await;
        }
    }

    pub(crate) async fn recv_async(&mut self) -> Option<UpstreamSseFramePumpItem> {
        self.active_storage = None;
        let item = self.rx.recv().await?;
        self.active_storage = item.storage;
        Some(item.item)
    }

    pub(crate) async fn recv_timeout_async(
        &mut self,
        timeout: Duration,
    ) -> Result<UpstreamSseFramePumpItem, RecvTimeoutError> {
        match tokio::time::timeout(timeout, self.recv_async()).await {
            Ok(Some(item)) => Ok(item),
            Ok(None) => Err(RecvTimeoutError::Disconnected),
            Err(_) => Err(RecvTimeoutError::Timeout),
        }
    }
}

#[cfg(test)]
#[path = "sse_frame_buffer_tests.rs"]
mod tests;

async fn send_frame(
    tx: &mpsc::Sender<QueuedItem>,
    queued: &Arc<Semaphore>,
    frame: &mut FrameBuffer,
    upstream: &mut GatewayByteStream,
    control: &mut InputControl,
) -> bool {
    let (lines, bytes, storage) = frame.take();
    let permit = loop {
        tokio::select! {
            biased;
            _ = tx.closed() => return false,
            _ = &mut control.stop, if !control.stopped => control.stop(upstream),
            permit = queued.clone().acquire_many_owned(bytes as u32) => break match permit { Ok(permit) => permit, Err(_) => return false },
        }
    };
    let slot = loop {
        tokio::select! {
            biased;
            _ = &mut control.stop, if !control.stopped => control.stop(upstream),
            slot = tx.reserve() => break match slot { Ok(slot) => slot, Err(_) => return false },
        }
    };
    slot.send(QueuedItem {
        item: UpstreamSseFramePumpItem::Frame(lines),
        storage,
        _queued: Some(permit),
    });
    true
}
