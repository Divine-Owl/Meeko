// ╭──────────────────────────────────────────────────────────────────────────╮
// │   H O S T                                                                │
// │   meeko-hookd · loopback-pinned client of the Codync host (spec §6.3)    │
// │                                                                          │
// │   SSE /events?since=<rev> · POST /api/<method> · Bearer ~/.codync/token  │
// │   Jittered backoff 1s→30s · rev gap > cap ⇒ full resync · 401 ⇒ re-read  │
// │   GPL-3.0-only                                                           │
// ╰──────────────────────────────────────────────────────────────────────────╯

use crate::projection::{Projection, Push, MAX_REV_GAP};
use crate::token::Token;
use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::header::{AUTHORIZATION, CONTENT_TYPE, HOST};
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::time::{sleep, timeout};

const BACKOFF_MIN_MS: u64 = 1000;
const BACKOFF_MAX_MS: u64 = 30_000;
/// No live line from the host for this long ⇒ assume offline (debounce).
const OFFLINE_DEBOUNCE: Duration = Duration::from_secs(5);

/// Events this build knows how to project. Anything else is undecodable:
/// keep the projection, log once per kind (host newer ⇒ link.degraded).
const KNOWN_EVENTS: &[&str] = &[
    "hello",
    "bot",
    "entry",
    "accessRequests",
    "usage",
    "screen",
    "cloud",
    "resync",
];

#[derive(Clone)]
pub struct Host {
    base: String,
    token: Arc<Token>,
}

impl Host {
    pub fn new(base: String, token: Arc<Token>) -> Self {
        Host { base, token }
    }

    async fn connect(&self) -> Result<TokioIo<TcpStream>, String> {
        // Base URL is pinned loopback at config time; parse host:port from it.
        let addr = self
            .base
            .strip_prefix("http://")
            .ok_or_else(|| format!("unsupported host url {}", self.base))?
            .trim_end_matches('/');
        let stream = TcpStream::connect(addr)
            .await
            .map_err(|e| format!("connect {addr}: {e}"))?;
        Ok(TokioIo::new(stream))
    }

    /// host:port for the Host header (origin-form requests over raw conn).
    fn authority(&self) -> &str {
        self.base
            .strip_prefix("http://")
            .unwrap_or(&self.base)
            .trim_end_matches('/')
    }

    fn authorize(&self, req: hyper::http::request::Builder, token: &str) -> Request<Empty<Bytes>> {
        req.header(AUTHORIZATION, format!("Bearer {token}"))
            .body(Empty::new())
            .expect("static request builder cannot fail")
    }

    /// POST /api/<method> with a JSON body. Returns the host's JSON reply.
    /// Errors are coarse strings — never include the token or full payloads.
    pub async fn call(&self, method: &str, body: Value) -> Result<Value, String> {
        // Methods are a fixed vocabulary; sanitize before interpolation.
        if !method
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err("bad method".into());
        }
        let token = self.token.get().map_err(|e| format!("token: {e}"))?;
        let io = self.connect().await?;
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|e| format!("handshake: {e}"))?;
        tokio::spawn(async move {
            let _ = conn.await;
        });

        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/{}", method))
            .header(HOST, self.authority())
            .header(CONTENT_TYPE, "application/json");
        let req = req
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .body(http_body_util::Full::new(Bytes::from(body.to_string())).boxed())
            .map_err(|e| format!("build request: {e}"))?;

        let mut res = sender
            .send_request(req)
            .await
            .map_err(|e| format!("send: {e}"))?;
        if res.status() == StatusCode::UNAUTHORIZED {
            self.token.invalidate();
            return Err("host rejected token (401)".into());
        }
        let bytes = res
            .body_mut()
            .collect()
            .await
            .map_err(|e| format!("read body: {e}"))?
            .to_bytes();
        serde_json::from_slice(&bytes).map_err(|e| format!("host reply not json: {e}"))
    }

    /// SSE subscriber. Streams projected pushes into `tx` until the stream
    /// dies; the caller loops with backoff. Returns the last rev seen.
    pub async fn subscribe(
        &self,
        mut since: i64,
        projection: Arc<tokio::sync::RwLock<Projection>>,
        tx: &tokio::sync::broadcast::Sender<Push>,
    ) -> Result<i64, String> {
        let token = self.token.get().map_err(|e| format!("token: {e}"))?;
        let io = self.connect().await?;
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|e| format!("handshake: {e}"))?;
        tokio::spawn(async move {
            let _ = conn.await;
        });

        let req = self.authorize(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/events?since={}", since))
                .header(HOST, self.authority()),
            &token,
        );
        let res = sender
            .send_request(req)
            .await
            .map_err(|e| format!("send: {e}"))?;
        if res.status() == StatusCode::UNAUTHORIZED {
            self.token.invalidate();
            return Err("host rejected token (401)".into());
        }

        let mut body = res.into_body();
        let mut line = String::new();
        loop {
            let frame = timeout(OFFLINE_DEBOUNCE * 6, body.frame())
                .await
                .map_err(|_| "stream idle".to_string())?;
            let Some(frame) = frame else { break };
            let frame = frame.map_err(|e| format!("stream: {e}"))?;
            let Some(data) = frame.into_data().ok() else {
                continue;
            };

            let text = String::from_utf8_lossy(&data);
            for raw in text.split('\n') {
                let raw = raw.trim_end_matches('\r');
                let Some(payload) = raw.strip_prefix("data:").map(str::trim_start) else {
                    continue; // event:/id:/keepalive comments — ignored
                };
                if payload.is_empty() {
                    continue;
                }
                line.clear();
                line.push_str(payload);

                let Ok(event) = serde_json::from_str::<Value>(&line) else {
                    continue; // undecodable frame: drop, never crash
                };

                // Resync signal from the host's lag detector.
                if event.get("type").and_then(Value::as_str) == Some("resync") {
                    projection.write().await.reset();
                    return Ok(0);
                }

                let mut proj = projection.write().await;
                let event_rev = event.get("rev").and_then(Value::as_i64).unwrap_or(since);
                if event_rev - since > MAX_REV_GAP {
                    eprintln!(
                        "meeko-hookd: rev gap {since}→{event_rev} > {MAX_REV_GAP}, full resync"
                    );
                    proj.reset();
                    drop(proj);
                    // Advance the cursor to the current rev so the next poll
                    // starts from here, not from 0 (which would re-trigger the
                    // same gap check forever).
                    return Ok(event_rev);
                }
                since = since.max(event_rev);

                if !KNOWN_EVENTS.contains(&event.get("type").and_then(Value::as_str).unwrap_or(""))
                {
                    // Host newer than this build: keep projection, surface degraded.
                    let link = proj.set_link(crate::projection::LinkState::Degraded);
                    drop(proj);
                    let _ = tx.send(link);
                    continue;
                }

                let pushes = proj.apply(&event);
                drop(proj);
                for p in pushes {
                    // Broadcast: Err = zero receivers (shell not running),
                    // which is normal — never treat it as a stream failure.
                    let _ = tx.send(p);
                }
            }
        }
        Ok(since)
    }
}

/// Full host loop: subscribe → resubscribe with jittered backoff forever.
/// Emits Link(Offline) after a 5s debounce of silence (spec §6.4 flow 2).
pub async fn run(
    host: Host,
    projection: Arc<tokio::sync::RwLock<Projection>>,
    tx: broadcast::Sender<Push>,
) {
    let mut since: i64 = 0;
    let mut attempt: u32 = 0;
    loop {
        let began = std::time::Instant::now();
        match host.subscribe(since, projection.clone(), &tx).await {
            Ok(next) => {
                since = next;
                // A stream that ends instantly (bad status, empty body) is a
                // failure in disguise — back off instead of hot-looping.
                if began.elapsed() < Duration::from_secs(1) {
                    attempt = attempt.saturating_add(1);
                    let base = BACKOFF_MIN_MS.saturating_mul(1u64 << attempt.min(10));
                    sleep(fastrand_in(1..=base.min(BACKOFF_MAX_MS))).await;
                } else {
                    attempt = 0;
                }
            }
            Err(e) => {
                eprintln!("meeko-hookd: host stream ended: {e}");
                attempt = attempt.saturating_add(1);
                // Debounced offline after the first failure.
                if attempt == 1 {
                    let mut proj = projection.write().await;
                    let link = proj.set_link(crate::projection::LinkState::Offline);
                    drop(proj);
                    // Err = zero receivers = shell not running; normal.
                    let _ = tx.send(link);
                }
                let base = BACKOFF_MIN_MS.saturating_mul(1u64 << attempt.min(10));
                let capped = base.min(BACKOFF_MAX_MS);
                // +/- 20% jitter so a host restart doesn't sync-stamp reconnects.
                let jitter = capped / 5;
                let span = (capped - jitter).max(1)..=(capped + jitter);
                let wait = fastrand_in(span);
                sleep(wait).await;
            }
        }
    }
}

/// Tiny dependency-free jitter source (wall clock + pid), not for security.
fn fastrand_in(range: std::ops::RangeInclusive<u64>) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ (d.as_secs() << 13))
        .unwrap_or(0)
        ^ (std::process::id() as u64) << 7;
    let width = range.end() - range.start() + 1;
    Duration::from_millis(range.start() + (nanos % width))
}
