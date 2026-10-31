//! poll.rs — the LISTEN act for seats that cannot hold a stream, absorbed
//! from paw/pi-ear.py (2026-10-29, the python retirement). Plain
//! request/response MCP: initialize handshake once per session, `pulse`
//! every N seconds, cursor = newest `at` delivered — catch-up once on first
//! boot, never a redelivery after. The pattern measured as the only one
//! tunnels and five-volt hardware both survive.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use crate::adapters::append_jsonl;
use crate::{framed, wakes, Config};

pub struct McpClient {
    base: String,
    client: reqwest::Client,
    session: Option<String>,
    next_id: u64,
}

impl McpClient {
    pub fn new(base: &str) -> McpClient {
        McpClient {
            base: base.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(20))
                .build()
                .expect("reqwest client build"),
            session: None,
            next_id: 0,
        }
    }

    fn bump(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    async fn post(&mut self, payload: &Value) -> Result<Option<Value>, String> {
        let mut req = self
            .client
            .post(format!("{}/mcp", self.base))
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(serde_json::to_string(payload).map_err(|e| format!("encode: {e}"))?);
        if let Some(sid) = &self.session {
            req = req.header("mcp-session-id", sid);
        }
        let res = req.send().await.map_err(|e| format!("post: {e}"))?;
        if let Some(sid) = res.headers().get("mcp-session-id") {
            if let Ok(s) = sid.to_str() {
                self.session = Some(s.to_string());
            }
        }
        let raw = res.text().await.map_err(|e| format!("body: {e}"))?;
        Ok(find_id_result(&raw, payload))
    }

    /// pi-ear's re-handshake: a session lost is re-helloed, never fatal.
    pub async fn hello(&mut self, nick: &str) -> Result<(), String> {
        self.session = None;
        let init = serde_json::json!({
            "jsonrpc": "2.0", "id": self.bump(), "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26", "capabilities": {},
                "clientInfo": {"name": format!("chorux-ear:{nick}"), "version": "0.1.0"}
            }
        });
        if self.post(&init).await?.is_none() {
            return Err("initialize failed (no id-matched response)".into());
        }
        let note = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let _ = self.post(&note).await;
        Ok(())
    }

    pub async fn pulse(&mut self, group: &str) -> Result<Vec<Value>, String> {
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": self.bump(), "method": "tools/call",
            "params": {"name": "pulse", "arguments": {"group": group}}
        });
        let r = self
            .post(&req)
            .await?
            .ok_or_else(|| "pulse failed (no id-matched response)".to_string())?;
        let text = r
            .pointer("/result/content/0/text")
            .and_then(|t| t.as_str())
            .ok_or_else(|| "pulse failed (no result content[0].text)".to_string())?;
        let parsed: Value =
            serde_json::from_str(text).map_err(|e| format!("pulse text not JSON: {e}"))?;
        Ok(parsed
            .get("recent")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default())
    }
}

/// pi-ear's response parse, verbatim in shape: the MCP endpoint answers
/// SSE-framed (data: blocks per response) or raw JSON; the id-matched frame
/// is the reply, everything else is ignored.
fn find_id_result(raw: &str, payload: &Value) -> Option<Value> {
    let want = payload.get("id")?.as_u64()?;
    for block in raw.split("\n\n") {
        let data: String = block
            .lines()
            .filter(|l| l.starts_with("data:"))
            .map(|l| l[5..].trim())
            .collect::<Vec<_>>()
            .join("");
        if data.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(&data) {
            if v.get("id").and_then(|i| i.as_u64()) == Some(want) {
                return Some(v);
            }
        }
    }
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        if v.get("id").and_then(|i| i.as_u64()) == Some(want) {
            return Some(v);
        }
    }
    None
}

/// The poll seat: one loop, one cursor, the same poke law as the SSE half —
/// the room line is delivered QUOTED, spooled, hooked, injected; every
/// refusal dead-letters. Cursor advances only after the tick's deliveries,
/// so a crashed tick re-delivers at most its own mentions (dead.jsonl holds
/// the refusals either way).
pub async fn poll_loop(cfg: &Config, spool: &PathBuf, dead: &PathBuf) {
    let cursor_file = cfg.state.join(format!("cursor-{}", cfg.room));
    let every = Duration::from_secs(cfg.poll_every.max(1));
    let mut mcp = McpClient::new(&cfg.base);
    if let Err(e) = mcp.hello(&cfg.nick).await {
        eprintln!("chorux-ear: poll: hello failed: {e} — re-hello each breath");
    }
    eprintln!(
        "chorux-ear: polling {}/mcp pulse {} every {every:?} as @{}",
        cfg.base, cfg.room, cfg.nick
    );
    loop {
        match mcp.pulse(&cfg.room).await {
            Ok(recent) => {
                let cursor = tokio::fs::read_to_string(&cursor_file)
                    .await
                    .unwrap_or_default();
                let mut newest = cursor.clone();
                for say in recent {
                    let at = say.get("at").and_then(|a| a.as_str()).unwrap_or("");
                    if at.is_empty() || at <= cursor.as_str() {
                        continue;
                    }
                    if at > newest.as_str() {
                        newest = at.to_string();
                    }
                    let text = say.get("text").and_then(|t| t.as_str()).unwrap_or("");
                    let from = say.get("handle").and_then(|h| h.as_str()).unwrap_or("?");
                    if !wakes(text, &cfg.nick, &cfg.also) {
                        continue;
                    }
                    let frame = framed(from, text);
                    eprintln!("chorux-ear: WAKE — {frame}");
                    append_jsonl(
                        spool,
                        &serde_json::json!({
                            "from": from, "at": at,
                            "wake": format!("@{}", cfg.nick),
                            "text": text, "framed": frame, "source": "poll",
                        }),
                    )
                    .await;
                    for adapter in &cfg.injects {
                        adapter.deliver(&say, &frame, dead).await;
                    }
                }
                if newest != cursor {
                    let _ = tokio::fs::write(&cursor_file, &newest).await;
                }
            }
            Err(e) => {
                eprintln!("chorux-ear: poll: {e}; re-hello next breath");
                if let Err(e2) = mcp.hello(&cfg.nick).await {
                    eprintln!("chorux-ear: poll: re-hello failed: {e2}");
                }
            }
        }
        tokio::time::sleep(every).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_id_matched_frame_in_sse_framed_body() {
        let payload = serde_json::json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call"});
        let raw = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":6,\"result\":{\"x\":1}}\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"pulse\":true}}\n\n";
        let v = find_id_result(raw, &payload).expect("id 7 frame");
        assert_eq!(v["result"]["pulse"], true);
    }

    #[test]
    fn finds_id_matched_frame_in_raw_json_body() {
        let payload = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"});
        let raw = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}";
        assert!(find_id_result(raw, &payload).is_some());
    }

    #[test]
    fn notification_payload_yields_none() {
        let payload = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(find_id_result("data: {}\n\n", &payload).is_none());
    }

    #[test]
    fn unmatched_id_yields_none() {
        let payload = serde_json::json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call"});
        assert!(find_id_result("data: {\"id\":8}\n\n", &payload).is_none());
    }
}
