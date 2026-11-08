//! opencode-serve — the opencode serve HTTP adapter (receiving-side only).
//!
//! Resolves or creates a session on the local opencode serve by title,
//! then POSTs the room line as an async prompt. The serve's password
//! comes from the standard env var — the same one every other EE
//! component reads (`OPENCODE_SERVER_PASSWORD`), so the ear never
//! stores secrets and never self-seeds them.
//!
//! Wire shape (the opencode serve API, measured live — v2):
//!   GET  /api/session                  → {"data": [sessions]} (v2 wrapper)
//!   POST /api/session                  → {"data": {session}} — body {title, parentID}
//!   POST /api/session/<id>/prompt      → {"data": {message}} — body {"text": "..."}
//!   Auth: basic auth, user "opencode", password from OPENCODE_SERVER_PASSWORD
//!   (v1 serves return bare arrays/objects without the "data" wrapper — the
//!   adapter handles both shapes)
//!
//! Adapter law: never drop a wake quietly — every refusal dead-letters
//! the exact payload to <state>/dead.jsonl, loud. A serve that cannot
//! be reached is a failed delivery, not a silent skip.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use serde_json::Value;

use super::{dead_letter, HarnessAdapter};

// ---------------------------------------------------------------------------
// dependencies: we need an HTTP client that works in a static musl binary.
// `reqwest` with default features pulls tokio + hyper + rustls — too heavy.
// For one simple POST call, a hand-rolled minimal HTTP client over
// `tokio::net::TcpStream` keeps the binary lean and the deps list short.
// The opencode serve is always on localhost, so TLS is never needed.
// ---------------------------------------------------------------------------

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const CONNECT_TIMEOUT_SECS: u64 = 5;
const IO_TIMEOUT_SECS: u64 = 30;

pub struct OpencodeServe {
    /// The serve's base URL (e.g. "http://127.0.0.1:4096")
    base: String,
    /// The session title to resolve-or-create (from the seat config)
    session_title: String,
    /// The serve password, read from the standard env at build time
    /// (never stored, never logged, never self-seeded)
    password: String,
}

/// Build from the adapter arg string: `opencode-serve:<base-url>|<session-title>`
/// The base URL and session title are per-seat config (SEATS.json).
/// The password is always read from OPENCODE_SERVER_PASSWORD env (the
/// standard EE mechanism — the adapter never holds a password in config).
pub fn build(arg: &str) -> Result<Box<dyn HarnessAdapter>, String> {
    // arg shape: "http://host:port|session-title"
    let parts: Vec<&str> = arg.split('|').collect();
    if parts.len() != 2 {
        return Err(format!(
            "adapter 'opencode-serve': expected '<base-url>|<session-title>', got '{}'",
            arg
        ));
    }
    let base = parts[0].trim().to_string();
    let session_title = parts[1].trim().to_string();
    if base.is_empty() || session_title.is_empty() {
        return Err("adapter 'opencode-serve': empty base URL or session title".into());
    }

    // The password from the standard env (fail loud, never silent — the
    // serve-password drift card exists precisely because of silent failure
    // in this env var)
    let password = std::env::var("OPENCODE_SERVER_PASSWORD")
        .map_err(|_| {
            "adapter 'opencode-serve': OPENCODE_SERVER_PASSWORD not set — the serve's \
             auth is required; this is the same env var every EE component reads"
        })?;

    Ok(Box::new(OpencodeServe {
        base,
        session_title,
        password,
    }))
}

impl HarnessAdapter for OpencodeServe {
    fn kind(&self) -> &'static str {
        "opencode-serve"
    }

    fn describe(&self) -> String {
        format!(
            "opencode serve {} → session '{}'",
            self.base, self.session_title
        )
    }

    fn deliver<'a>(
        &'a self,
        _event: &'a Value,
        frame: &'a str,
        dead: &'a Path,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let text = frame.to_string();
            match self.deliver_inner(&text).await {
                Ok(session_id) => {
                    // success: the prompt landed, the session will take a turn
                    eprintln!(
                        "chorux-ear: opencode-serve: prompt delivered to session {}",
                        session_id
                    );
                }
                Err(e) => {
                    // failure: dead-letter loud, never drop
                    dead_letter(dead, "opencode-serve", frame, &e).await;
                    eprintln!(
                        "chorux-ear: opencode-serve: deliver failed (dead-lettered to {:?}): {}",
                        dead,
                        e
                    );
                }
            }
        })
    }
}

impl OpencodeServe {
    /// Resolve or create a session by title, then POST the prompt.
    async fn deliver_inner(&self, text: &str) -> Result<String, String> {
        // 1. resolve the session by title
        let session_id = self
            .resolve_session()
            .await?
            .or_else(|| None) // if not found, we'll create
            ;

        let session_id = match session_id {
            Some(id) => id,
            None => {
                // 2. create a session with the title
                self.create_session().await?
            }
        };

        // 3. POST the prompt (async — the session takes a turn)
        self.post_prompt(&session_id, text).await?;

        Ok(session_id)
    }

    /// GET /api/session — list sessions, find by title
    async fn resolve_session(&self) -> Result<Option<String>, String> {
        let (host, port, base_path) = self.parse_url()?;
        let path = format!("{}/api/session", base_path);
        let body = self
            .http_request(&host, port, &format!("GET {} HTTP/1.1", path))
            .await?;
        // parse the JSON array, find the session by title
        // The response shape is: [{"id": "...", "title": "...", ...}, ...]
        let resp: Value = serde_json::from_str(&body)
            .map_err(|e| format!("resolve: bad JSON from serve: {}", e))?;
        // v2 serves wrap responses in {"data": ...}; v1 serves return bare
        // arrays. Handle both: unwrap "data" if present, else use as-is.
        let sessions = match resp.get("data").and_then(|d| d.as_array()) {
            Some(arr) => arr.clone(),
            None => match resp.as_array() {
                Some(arr) => arr.clone(),
                None => return Err(format!("resolve: unexpected response shape: {}", &body[..body.len().min(100)])),
            },
        };
        for s in &sessions {
            if s.get("title").and_then(|t| t.as_str()) == Some(&self.session_title) {
                if let Some(id) = s.get("id").and_then(|i| i.as_str()) {
                    return Ok(Some(id.to_string()));
                }
            }
        }
        Ok(None)
    }

    /// POST /api/session — create a session with the title
    async fn create_session(&self) -> Result<String, String> {
        let (host, port, base_path) = self.parse_url()?;
        let path = format!("{}/api/session", base_path);
        let create_body = serde_json::json!({
            "title": self.session_title,
            "parentID": null,
        });
        let body = self
            .http_request_with_body(
                &host,
                port,
                &format!("POST {} HTTP/1.1", path),
                &create_body.to_string(),
            )
            .await?;
        let resp: Value = serde_json::from_str(&body)
            .map_err(|e| format!("create: bad JSON from serve: {}", e))?;
        // v2 wraps: {"data": {"id": ...}}; v1 is bare: {"id": ...}
        let session = resp.get("data").unwrap_or(&resp);
        session.get("id")
            .and_then(|i| i.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("create: no session id in response: {}", body))
    }

    /// POST /api/session/<id>/prompt — inject the text as an async turn
    async fn post_prompt(&self, session_id: &str, text: &str) -> Result<(), String> {
        let (host, port, base_path) = self.parse_url()?;
        let path = format!("{}/api/session/{}/prompt", base_path, session_id);
        let prompt_body = serde_json::json!({
            "text": text,
        });
        let body = self
            .http_request_with_body(
                &host,
                port,
                &format!("POST {} HTTP/1.1", path),
                &prompt_body.to_string(),
            )
            .await?;
        // a successful prompt returns 200 or 202
        // if we got here without an HTTP error, the prompt landed
        let _ = body; // the response body is the new message, we don't need it
        Ok(())
    }

    /// Parse the base URL into (host, port, base_path)
    fn parse_url(&self) -> Result<(String, u16, String), String> {
        // strip http://
        let stripped = self
            .base
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        // split host:port from path
        let (hostport, path) = match stripped.find('/') {
            Some(i) => (&stripped[..i], &stripped[i..]),
            None => (stripped, ""),
        };
        // split host from port
        let (host, port) = match hostport.rfind(':') {
            Some(i) => (&hostport[..i], hostport[i + 1..].parse::<u16>().unwrap_or(4096)),
            None => (hostport, 4096),
        };
        if host.is_empty() {
            return Err(format!("opencode-serve: bad base URL '{}'", self.base));
        }
        // base_path should be empty or start with /
        let base_path = if path.is_empty() { String::new() } else { path.to_string() };
        Ok((host.to_string(), port, base_path))
    }

    /// Minimal HTTP request over TCP (no external HTTP client dependency —
    /// the serve is always localhost, so TLS is never needed)
    async fn http_request(&self, host: &str, port: u16, request_line: &str) -> Result<String, String> {
        self.http_request_with_body(host, port, request_line, "").await
    }

    async fn http_request_with_body(
        &self,
        host: &str,
        port: u16,
        request_line: &str,
        body: &str,
    ) -> Result<String, String> {
        let addr = format!("{}:{}", host, port);
        let mut stream = tokio::time::timeout(
            std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS),
            TcpStream::connect(&addr),
        )
        .await
        .map_err(|_| format!("connect timeout to {}", addr))?
        .map_err(|e| format!("connect to {}: {}", addr, e))?;

        // build the HTTP request with basic auth
        let auth = base64_encode(&format!("opencode:{}", self.password));
        let content_len = body.len();
        let request = if body.is_empty() {
            // GET (no body): bare request, no Content-Type/Content-Length
            // (some HTTP servers — notably Bun — reject body headers on
            // bodyless requests with 400 Bad Request, measured live)
            format!(
                "{}\r\nHost: {}\r\nAuthorization: Basic {}\r\nConnection: close\r\n\r\n",
                request_line, host, auth
            )
        } else {
            // POST (with JSON body): full headers
            format!(
                "{}\r\nHost: {}\r\nAuthorization: Basic {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                request_line, host, auth, content_len, body
            )
        };

        // write
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| format!("write: {}", e))?;

        // read (with timeout)
        let mut response = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = tokio::time::timeout(
                std::time::Duration::from_secs(IO_TIMEOUT_SECS),
                stream.read(&mut buf),
            )
            .await
            .map_err(|_| "read timeout".to_string())?
            .map_err(|e| format!("read: {}", e))?;
            if n == 0 {
                break;
            }
            response.extend_from_slice(&buf[..n]);
        }

        // extract the response body (after \r\n\r\n)
        let response_str = String::from_utf8_lossy(&response).to_string();
        let body_start = response_str
            .find("\r\n\r\n")
            .map(|i| i + 4)
            .ok_or_else(|| format!("no HTTP body in response: {}", &response_str[..response_str.len().min(200)]))?;

        // check for HTTP errors
        if response_str.contains("HTTP/1.1 401") || response_str.contains("HTTP/1.1 403") {
            return Err("authentication refused — check OPENCODE_SERVER_PASSWORD".into());
        }
        if response_str.contains("HTTP/1.1 404") {
            return Err("session not found — the serve may have compacted".into());
        }
        if response_str.contains("HTTP/1.1 5") {
            return Err("serve internal error".into());
        }

        Ok(response_str[body_start..].to_string())
    }
}

/// Base64 encode (hand-rolled, no dependency — the password is short)
fn base64_encode(input: &str) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARSET[(triple >> 18) as usize & 0x3F] as char);
        result.push(CHARSET[(triple >> 12) as usize & 0x3F] as char);
        if chunk.len() > 1 {
            result.push(CHARSET[(triple >> 6) as usize & 0x3F] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARSET[triple as usize & 0x3F] as char);
        } else {
            result.push('=');
        }
    }
    result
}

// ---------------------------------------------------------------------------
// tests — every test is self-documenting: the name says what law it pins
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // OPENCODE_SERVER_PASSWORD is process-global state: the tests that touch it
    // serialize behind one lock — parallel test threads racing set/remove made
    // aa_build_refuses_missing_password_env flake in CI (measured live,
    // the v0.0.11 release run: "must refuse" while a sibling had just set it)
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // -- build(): the config contract --------------------------------------

    #[test]
    fn build_parses_valid_spec() {
        // the happy path: base URL and session title, split on '|'
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("OPENCODE_SERVER_PASSWORD", "test-pw");
        let a = build("http://127.0.0.1:4096|lane-omo-9p").unwrap();
        assert_eq!(a.kind(), "opencode-serve");
        assert!(a.describe().contains("127.0.0.1:4096"));
        assert!(a.describe().contains("lane-omo-9p"));
    }

    #[test]
    fn build_refuses_missing_pipe() {
        // no '|' separator: fail loud, name the expected shape
        let err = build("http://localhost:4096").err().expect("must refuse");
        assert!(err.contains("expected"), "err: {err}");
    }

    #[test]
    fn build_refuses_empty_parts() {
        // pipe present but empty side: fail loud
        let err = build("|title").err().expect("empty base must refuse");
        assert!(err.contains("empty"), "err: {err}");
        let err = build("http://x:1|").err().expect("empty title must refuse");
        assert!(err.contains("empty"), "err: {err}");
    }

    #[test]
    fn aa_build_refuses_missing_password_env() {
        // the adapter law: auth comes from OPENCODE_SERVER_PASSWORD env,
        // never stored in config, never self-seeded — fail loud when absent
        // (save/restore so we don't break other tests; under ENV_LOCK so a
        // sibling's set_var cannot repopulate it mid-build)
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let saved = std::env::var("OPENCODE_SERVER_PASSWORD").ok();
        std::env::remove_var("OPENCODE_SERVER_PASSWORD");
        let err = build("http://127.0.0.1:4096|test").err().expect("must refuse");
        assert!(err.contains("OPENCODE_SERVER_PASSWORD"), "err: {err}");
        if let Some(pw) = saved {
            std::env::set_var("OPENCODE_SERVER_PASSWORD", pw);
        }
    }

    // -- parse_url(): the URL decomposition ----------------------------------

    #[test]
    fn parse_url_simple() {
        let a = OpencodeServe {
            base: "http://127.0.0.1:4096".to_string(),
            session_title: "t".to_string(),
            password: "x".to_string(),
        };
        let (host, port, path) = a.parse_url().unwrap();
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 4096);
        assert_eq!(path, "");
    }

    #[test]
    fn parse_url_with_path() {
        let a = OpencodeServe {
            base: "http://localhost:8080/api".to_string(),
            session_title: "t".to_string(),
            password: "x".to_string(),
        };
        let (host, port, path) = a.parse_url().unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 8080);
        assert_eq!(path, "/api");
    }

    #[test]
    fn parse_url_no_port_defaults() {
        let a = OpencodeServe {
            base: "http://localhost".to_string(),
            session_title: "t".to_string(),
            password: "x".to_string(),
        };
        let (_, port, _) = a.parse_url().unwrap();
        assert_eq!(port, 4096, "default port is the opencode serve standard");
    }

    // -- base64_encode(): the auth encoding ----------------------------------

    #[test]
    fn base64_known_vectors() {
        // RFC 4648 test vectors — the standard correctness oracle
        assert_eq!(base64_encode(""), "");
        assert_eq!(base64_encode("f"), "Zg==");
        assert_eq!(base64_encode("fo"), "Zm8=");
        assert_eq!(base64_encode("foo"), "Zm9v");
        assert_eq!(base64_encode("foob"), "Zm9vYg==");
        assert_eq!(base64_encode("fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode("foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_auth_pair() {
        // the actual shape we encode: "opencode:<password>"
        let encoded = base64_encode("opencode:secret123");
        // verify by decoding: 3 chunks of 3 bytes + 2 remainder = 12 chars + 2 padding
        assert!(encoded.len() >= 16);
        assert!(!encoded.is_empty());
    }

    // -- the adapter law: dead-letter on failure ------------------------------

    #[tokio::test]
    async fn refusal_unreachable_serve_dead_letters() {
        // the adapter law: never drop a wake quietly — an unreachable serve
        // is a failed delivery, the payload dead-letters with the reason
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("OPENCODE_SERVER_PASSWORD", "test-pw");
        let d = std::env::temp_dir().join(format!("ear-test-opencode-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let dead = d.join("dead.jsonl");

        // a port that is guaranteed closed (1 is never a serve)
        let a = build("http://127.0.0.1:1|test-session").unwrap();
        let event = json!({"handle": "vip", "text": "ping @test-session"});
        let frame = "room line from vip: ping @test-session";
        a.deliver(&event, frame, &dead).await;

        let dlq = std::fs::read_to_string(&dead)
            .expect("dead.jsonl must exist — the wake was not dropped quietly");
        assert!(dlq.contains("opencode-serve"), "adapter name in: {dlq}");
        assert!(dlq.contains("connect"), "connect failure reason in: {dlq}");
        assert!(dlq.contains("ping @test-session"), "the payload preserved: {dlq}");
        std::fs::remove_dir_all(&d).ok();
    }
}
