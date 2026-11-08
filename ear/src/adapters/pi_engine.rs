//! pi-engine — the omo/senpi runtime's own wake API: the TUI control socket.
//!
//! Wire shape, measured from the runtime's shipped client code
//! (`dist/modes/rpc/tui-socket.js`, `socket-transport.js`,
//! `rpc-monitor-observer.js`):
//!   - connect `<agentDir>/rpc/tui/t-<16hex>.sock`
//!   - the TUI control socket authenticates EVERY connection: send the raw
//!     32-byte secret from `<socket>.secret` first (the server times out in
//!     2s and destroys the connection on mismatch; any bytes after the
//!     secret are frames)
//!   - framing is newline-delimited JSON; a wake is
//!     `{"type":"prompt","message":…,"id":…}` and the answer is
//!     `{"type":"response","id":…,"success":true|false,"error"?}`
//!   - one frame per wake, one response, then this adapter drops the
//!     connection — a wake is not a seat.
//!
//! Adapter law: never drop a wake quietly — every refusal dead-letters the
//! exact payload to <state>/dead.jsonl, loud.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use super::{dead_letter, expand_arg, HarnessAdapter};

const SECRET_BYTES: usize = 32;
const CONNECT_TIMEOUT_SECS: u64 = 5;
const IO_TIMEOUT_SECS: u64 = 10;

pub struct PiEngine {
    socket: PathBuf,
}

pub fn build(arg: &str) -> Result<Box<dyn HarnessAdapter>, String> {
    Ok(Box::new(PiEngine {
        socket: expand_arg(arg, "pi-engine")?,
    }))
}

fn next_id() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("chorux-ear-{ms}")
}

impl HarnessAdapter for PiEngine {
    fn kind(&self) -> &'static str {
        "pi-engine"
    }
    fn describe(&self) -> String {
        format!("pi-engine socket {}", self.socket.display())
    }
    fn deliver<'a>(
        &'a self,
        _event: &'a Value,
        frame: &'a str,
        dead: &'a Path,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            match self.deliver_inner(frame).await {
                Ok(()) => {
                    eprintln!("chorux-ear: pi-engine: prompt accepted by {}", self.socket.display());
                }
                Err(reason) => {
                    eprintln!(
                        "chorux-ear: pi-engine deliver failed (dead-lettering to {}): {}",
                        dead.display(),
                        reason
                    );
                    dead_letter(dead, &self.describe(), frame, &reason).await;
                }
            }
        })
    }
}

impl PiEngine {
    async fn deliver_inner(&self, message: &str) -> Result<(), String> {
        let secret_path = PathBuf::from(format!("{}.secret", self.socket.display()));
        let secret = tokio::fs::read(&secret_path)
            .await
            .map_err(|e| format!("cannot read socket secret {}: {e}", secret_path.display()))?;
        if secret.len() != SECRET_BYTES {
            return Err(format!(
                "socket secret {} is {} bytes, want {SECRET_BYTES} — refusing to guess auth",
                secret_path.display(),
                secret.len()
            ));
        }

        let mut stream = tokio::time::timeout(
            std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS),
            UnixStream::connect(&self.socket),
        )
        .await
        .map_err(|_| format!("connect timeout to {}", self.socket.display()))?
        .map_err(|e| format!("connect to {}: {e}", self.socket.display()))?;

        stream
            .write_all(&secret)
            .await
            .map_err(|e| format!("handshake write: {e}"))?;

        let id = next_id();
        let line = format!(
            "{}\n",
            serde_json::to_string(&serde_json::json!({
                "type": "prompt",
                "message": message,
                "id": id,
                // the engine's own refusal names this field: a prompt against a
                // running turn is refused `session_busy ... names no
                // streamingBehavior` (measured live);
                // steer = queue at the next tool boundary, never interrupt
                "streamingBehavior": "steer",
            }))
            .map_err(|e| format!("encode frame: {e}"))?
        );
        stream
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("frame write: {e}"))?;

        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = tokio::time::timeout(
                std::time::Duration::from_secs(IO_TIMEOUT_SECS),
                stream.read(&mut chunk),
            )
            .await
            .map_err(|_| "response timeout — the engine never answered".to_string())?
            .map_err(|e| format!("read: {e}"))?;
            if n == 0 {
                return Err("connection closed before a response".into());
            }
            buf.extend_from_slice(&chunk[..n]);
            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
                let line_str = String::from_utf8_lossy(&line_bytes[..line_bytes.len() - 1]).trim().to_string();
                if line_str.is_empty() {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(&line_str) else {
                    continue;
                };
                if v.get("type").and_then(|t| t.as_str()) != Some("response") {
                    continue;
                }
                if v.get("id").and_then(|i| i.as_str()) != Some(id.as_str()) {
                    continue;
                }
                return if v.get("success").and_then(|s| s.as_bool()) == Some(true) {
                    Ok(())
                } else {
                    Err(format!(
                        "engine refused: {}",
                        v.get("error").and_then(|e| e.as_str()).unwrap_or("unknown error")
                    ))
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::net::UnixListener;

    fn temp_socket(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pi-engine-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("t-0123456789abcdef.sock")
    }

    #[test]
    fn build_parses_path_and_refuses_empty() {
        assert_eq!(build("/tmp/t.sock").unwrap().kind(), "pi-engine");
        let err = build("").err().expect("empty arg must refuse");
        assert!(err.contains("empty"), "err: {err}");
    }

    #[tokio::test]
    async fn full_wire_against_mock_engine() {
        let socket = temp_socket("wire");
        let secret_file = PathBuf::from(format!("{}.secret", socket.display()));
        let secret: Vec<u8> = (0..SECRET_BYTES).map(|i| i as u8).collect();
        std::fs::write(&secret_file, &secret).unwrap();

        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut got = vec![0u8; SECRET_BYTES];
            conn.read_exact(&mut got).await.unwrap();
            assert_eq!(got, secret, "the 32-byte secret must arrive first, raw");

            let mut line = Vec::new();
            let mut b = [0u8; 1];
            loop {
                conn.read_exact(&mut b).await.unwrap();
                if b[0] == b'\n' { break; }
                line.push(b[0]);
            }
            let frame: Value = serde_json::from_slice(&line).unwrap();
            let reply = json!({
                "type": "response",
                "id": frame["id"].as_str().unwrap(),
                "success": true,
            });
            conn.write_all(format!("{}\n", reply).to_string().as_bytes()).await.unwrap();
            (frame, got)
        });

        let adapter = build(socket.to_str().unwrap()).unwrap();
        let dead = socket.parent().unwrap().join("dead.jsonl");
        let event = json!({"handle": "pilot", "text": "ping @omo"});
        adapter.deliver(&event, "room line from pilot: ping @omo", &dead).await;

        let (frame, _) = server.await.unwrap();
        assert_eq!(frame["type"], "prompt");
        assert_eq!(frame["streamingBehavior"], "steer");
        assert_eq!(frame["message"], "room line from pilot: ping @omo");
        assert!(frame["id"].as_str().unwrap().starts_with("chorux-ear-"));
        assert!(!dead.exists(), "a delivered wake never dead-letters");

        let _ = std::fs::remove_file(&secret_file);
        let _ = std::fs::remove_file(&socket);
    }

    #[tokio::test]
    async fn engine_refusal_dead_letters_the_exact_payload() {
        let socket = temp_socket("refused");
        let secret_file = PathBuf::from(format!("{}.secret", socket.display()));
        std::fs::write(&secret_file, vec![7u8; SECRET_BYTES]).unwrap();

        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut got = vec![0u8; SECRET_BYTES];
            conn.read_exact(&mut got).await.unwrap();
            let mut line = Vec::new();
            let mut b = [0u8; 1];
            loop {
                conn.read_exact(&mut b).await.unwrap();
                if b[0] == b'\n' { break; }
                line.push(b[0]);
            }
            let frame: Value = serde_json::from_slice(&line).unwrap();
            let reply = json!({"type": "response", "id": frame["id"].as_str().unwrap(), "success": false, "error": "session_busy"});
            conn.write_all(format!("{}\n", reply).to_string().as_bytes()).await.unwrap();
        });

        let adapter = build(socket.to_str().unwrap()).unwrap();
        let dead = socket.parent().unwrap().join("dead.jsonl");
        adapter
            .deliver(&json!({"handle": "pilot", "text": "ping"}), "room line from pilot: ping", &dead)
            .await;

        server.await.unwrap();
        let dead_line = std::fs::read_to_string(&dead).unwrap();
        assert!(dead_line.contains("engine refused"), "dead.jsonl: {dead_line}");
        assert!(dead_line.contains("room line from pilot: ping"), "the exact payload is preserved: {dead_line}");

        let _ = std::fs::remove_file(&secret_file);
        let _ = std::fs::remove_file(&socket);
        let _ = std::fs::remove_file(&dead);
    }

    #[tokio::test]
    async fn wrong_secret_length_refuses_before_connecting() {
        let socket = temp_socket("badsecret");
        let secret_file = PathBuf::from(format!("{}.secret", socket.display()));
        std::fs::write(&secret_file, vec![1u8; 5]).unwrap();

        let adapter = build(socket.to_str().unwrap()).unwrap();
        let dead = socket.parent().unwrap().join("dead.jsonl");
        adapter
            .deliver(&json!({"handle": "pilot", "text": "x"}), "room line from pilot: x", &dead)
            .await;

        let dead_line = std::fs::read_to_string(&dead).unwrap();
        assert!(dead_line.contains("want 32"), "dead.jsonl: {dead_line}");

        let _ = std::fs::remove_file(&secret_file);
        let _ = std::fs::remove_file(&dead);
    }

    /// The live leg: run with a real TUI socket via
    /// `SENPI_TEST_PI_ENGINE_SOCKET=<sock> cargo test -p chorux-ear pi_engine -- --ignored`.
    /// Uses `get_protocol_info` (an observe-class method) — it proves wire,
    /// auth and framing without waking anything.
    #[tokio::test]
    #[ignore]
    async fn live_socket_answers_get_protocol_info() {
        let socket = match std::env::var("SENPI_TEST_PI_ENGINE_SOCKET") {
            Ok(s) => PathBuf::from(s),
            Err(_) => return,
        };
        let secret_path = PathBuf::from(format!("{}.secret", socket.display()));
        let secret = tokio::fs::read(&secret_path).await.unwrap();
        assert_eq!(secret.len(), SECRET_BYTES);
        let mut stream = UnixStream::connect(&socket).await.unwrap();
        stream.write_all(&secret).await.unwrap();
        let id = "chorux-ear-selftest";
        stream
            .write_all(format!("{}\n", json!({"type": "get_protocol_info", "id": id})).to_string().as_bytes())
            .await
            .unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let first_line = loop {
            let n = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut chunk))
                .await
                .expect("the engine answered within 5s")
                .unwrap();
            assert!(n > 0, "connection closed before a response");
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                break String::from_utf8_lossy(&buf[..pos]).to_string();
            }
        };
        let v: Value = serde_json::from_str(&first_line).unwrap();
        assert_eq!(v["id"], "chorux-ear-selftest", "raw first line: {first_line}");
        assert_eq!(v["success"], true, "raw first line: {first_line}");
    }
}
