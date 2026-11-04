//! Optional LAN browser viewer.
//!
//! A tiny_http server that lets any device on the same network open a browser
//! page and watch the live measurements. The acquisition thread publishes each
//! batch here; the server accumulates a small rolling snapshot (recent samples
//! and the latest batch) that browsers poll at `/state`. The served page is
//! self-contained (`remote_viewer.html`) and deliberately simpler than the
//! desktop UI.
//!
//! Why polling instead of SSE: tiny_http wraps response bodies in a buffered
//! chunked encoder that is only flushed when the full response is written, so
//! a long-lived streaming response is never delivered incrementally. Polling
//! `/state` every few hundred ms is simpler, robust, and loses no data because
//! the snapshot accumulates recent samples server-side.
//!
//! This module is a convenience, not a security boundary: the stream is
//! unauthenticated and only meant for a trusted LAN. The server binds
//! `0.0.0.0` so other machines can reach it; Windows Firewall may prompt on
//! first start.

use std::io::{self, Cursor};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tiny_http::{Header, Method, Response, Server, StatusCode};

/// The server thread polls for requests on this interval so it can notice a
/// stop request within ~200 ms without holding the listener.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

const REMOTE_VIEWER_HTML: &str = include_str!("../remote_viewer.html");

/// ECharts UMD build served to the browser page (kept in-repo so the Rust build
/// does not depend on node_modules; offline LAN, no CDN).
const ECHARTS_JS: &[u8] = include_bytes!("../echarts.min.js");

/// How many measurement samples the rolling snapshot keeps (~10 s at 50 Hz).
const MAX_SAMPLES: usize = 800;

#[derive(Clone)]
pub struct RemoteServer {
    inner: Arc<RemoteServerInner>,
}

struct RemoteServerInner {
    running: AtomicBool,
    url: Mutex<String>,
    snapshot: Arc<Mutex<Snapshot>>,
}

#[derive(Default)]
struct Snapshot {
    /// Rolling list of measurement objects from published batches.
    samples: Vec<Value>,
    /// The most recently published batch, used for readouts / stats / contract.
    latest: Option<Value>,
    connected: bool,
    label: String,
}

impl Default for RemoteServer {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteServer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RemoteServerInner {
                running: AtomicBool::new(false),
                url: Mutex::new(String::new()),
                snapshot: Arc::new(Mutex::new(Snapshot::default())),
            }),
        }
    }

    pub fn is_running(&self) -> bool {
        self.inner.running.load(Ordering::SeqCst)
    }

    pub fn url(&self) -> String {
        self.inner.url.lock().unwrap().clone()
    }

    /// Bind on 0.0.0.0:port and start serving. Returns an error when the port
    /// is unavailable.
    pub fn start(&self, port: u16) -> Result<(), String> {
        if self.is_running() {
            return Err("远程查看已在运行".into());
        }
        let server =
            Server::http(("0.0.0.0", port)).map_err(|e| format!("无法监听 {port} 端口：{e}"))?;

        let lan_ip = local_ip_address::local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|_| "127.0.0.1".to_string());
        let url = format!("http://{lan_ip}:{port}/");

        *self.inner.url.lock().unwrap() = url;
        self.inner.running.store(true, Ordering::SeqCst);

        let me = self.clone();
        thread::spawn(move || {
            while me.inner.running.load(Ordering::SeqCst) {
                match server.recv_timeout(POLL_INTERVAL) {
                    Ok(Some(request)) => {
                        let handle_me = me.clone();
                        thread::spawn(move || handle_me.handle(request));
                    }
                    Ok(None) => continue,
                    Err(_) => break,
                }
            }
            me.inner.running.store(false, Ordering::SeqCst);
        });

        Ok(())
    }

    /// Stop serving. Pending poll requests complete; the listener closes within
    /// ~POLL_INTERVAL.
    pub fn stop(&self) {
        self.inner.running.store(false, Ordering::SeqCst);
    }

    /// Record that a device is (or is not) connected so browsers can show it.
    pub fn set_connected(&self, connected: bool, label: String) {
        let mut snapshot = self.inner.snapshot.lock().unwrap();
        snapshot.connected = connected;
        snapshot.label = label;
    }

    /// Serialize `value` and fold it into the rolling snapshot. No-op while the
    /// server is stopped.
    pub fn publish<T: Serialize>(&self, value: &T) {
        if !self.is_running() {
            return;
        }
        let Ok(batch) = serde_json::to_value(value) else {
            return;
        };
        let mut snapshot = self.inner.snapshot.lock().unwrap();
        snapshot.latest = Some(batch.clone());

        if let Some(measurements) = batch.get("measurements").and_then(Value::as_array) {
            for m in measurements {
                snapshot.samples.push(m.clone());
            }
            let overflow = snapshot.samples.len().saturating_sub(MAX_SAMPLES);
            if overflow > 0 {
                snapshot.samples.drain(0..overflow);
            }
        }
    }

    fn handle(self, request: tiny_http::Request) {
        let result = if request.method() == &Method::Get {
            let path = request.url().split('?').next().unwrap_or("/");
            match path {
                "/" => self.respond_html(request),
                "/state" => self.respond_state(request),
                "/echarts.min.js" => self.respond_echarts(request),
                _ => request.respond(Response::empty(StatusCode(404))),
            }
        } else {
            request.respond(Response::empty(StatusCode(405)))
        };
        let _ = result;
    }

    fn respond_html(&self, request: tiny_http::Request) -> Result<(), io::Error> {
        let headers = vec![
            Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap(),
            Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).unwrap(),
        ];
        request.respond(Response::new(
            StatusCode(200),
            headers,
            Cursor::new(REMOTE_VIEWER_HTML.as_bytes()),
            Some(REMOTE_VIEWER_HTML.len()),
            None,
        ))
    }

    fn respond_echarts(&self, request: tiny_http::Request) -> Result<(), io::Error> {
        let headers = vec![
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/javascript; charset=utf-8"[..],
            )
            .unwrap(),
            Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).unwrap(),
        ];
        request.respond(Response::new(
            StatusCode(200),
            headers,
            Cursor::new(ECHARTS_JS.to_vec()),
            Some(ECHARTS_JS.len()),
            None,
        ))
    }

    fn respond_state(&self, request: tiny_http::Request) -> Result<(), io::Error> {
        let snapshot = self.inner.snapshot.lock().unwrap();
        let body = json!({
            "connected": snapshot.connected,
            "label": snapshot.label,
            "latest": snapshot.latest,
            "samples": snapshot.samples,
        });
        drop(snapshot);
        let bytes = serde_json::to_vec(&body).map_err(|e| io::Error::other(e.to_string()))?;
        let headers = vec![
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
            Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).unwrap(),
        ];
        let length = bytes.len();
        request.respond(Response::new(
            StatusCode(200),
            headers,
            Cursor::new(bytes),
            Some(length),
            None,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    fn free_port() -> u16 {
        std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn http_get(port: u16, path: &str) -> (u16, Vec<u8>) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req =
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let head_end = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|i| i + 4)
            .unwrap_or(raw.len());
        let status_line = String::from_utf8_lossy(
            &raw[..raw.iter().position(|&b| b == b'\r').unwrap_or(raw.len())],
        );
        let code: u16 = status_line
            .split_whitespace()
            .nth(1)
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        let body = raw[head_end..].to_vec();
        (code, body)
    }

    #[test]
    fn serves_html_and_404() {
        let port = free_port();
        let server = RemoteServer::new();
        server.start(port).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        let (code, body) = http_get(port, "/");
        assert_eq!(code, 200);
        assert!(String::from_utf8_lossy(&body).contains("Star monitor"));

        let (code, _) = http_get(port, "/nope");
        assert_eq!(code, 404);

        server.stop();
        std::thread::sleep(Duration::from_millis(300));
    }

    #[test]
    fn snapshot_accumulates_published_batches() {
        let port = free_port();
        let server = RemoteServer::new();
        server.start(port).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        server.set_connected(true, "KM003C (test)".into());
        server.publish(&json!({
            "measurements": [ { "device_ms": 1, "host_ms": 10, "vbus_v": 5.0, "ibus_a": 1.0 } ],
            "stats": { "ok": 1, "polls": 2 }
        }));
        server.publish(&json!({
            "measurements": [ { "device_ms": 2, "host_ms": 20, "vbus_v": 5.1, "ibus_a": 1.2 } ],
            "stats": { "ok": 2, "polls": 4 }
        }));
        std::thread::sleep(Duration::from_millis(50));

        let (code, body) = http_get(port, "/state");
        assert_eq!(code, 200);
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["connected"], true);
        assert_eq!(value["label"], "KM003C (test)");
        assert_eq!(
            value["samples"].as_array().unwrap().len(),
            2,
            "both measurements kept"
        );
        assert_eq!(value["latest"]["stats"]["ok"], 2, "latest batch wins");

        server.stop();
        std::thread::sleep(Duration::from_millis(300));
    }

    #[test]
    fn snapshot_bounds_rolling_arrays() {
        let port = free_port();
        let server = RemoteServer::new();
        server.start(port).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        for i in 0..(MAX_SAMPLES + 50) {
            server.publish(&json!({ "measurements": [ { "device_ms": i, "host_ms": i } ] }));
        }
        std::thread::sleep(Duration::from_millis(50));

        let (_, body) = http_get(port, "/state");
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["samples"].as_array().unwrap().len(), MAX_SAMPLES);

        server.stop();
        std::thread::sleep(Duration::from_millis(300));
    }

    #[test]
    fn serves_echarts_umd_bundle() {
        let port = free_port();
        let server = RemoteServer::new();
        server.start(port).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        let (code, body) = http_get(port, "/echarts.min.js");
        assert_eq!(code, 200);
        assert!(
            String::from_utf8_lossy(&body).contains("echarts"),
            "echarts UMD bundle served"
        );

        server.stop();
        std::thread::sleep(Duration::from_millis(300));
    }

    #[test]
    fn port_in_use_is_an_error() {
        let listener = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = RemoteServer::new();
        let result = server.start(port);
        assert!(result.is_err(), "binding an occupied port must fail");
    }
}
