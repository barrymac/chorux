//! mcp.rs — the room half: stock MCP streamable-HTTP client, poll.rs's
//! proven shape (initialize once, session-id echoed, notifications/
//! initialized, id-matched replies from SSE-framed or raw bodies). The
//! bridge calls exactly three tools: join, say, sayTo — the same surface
//! any external client gets (L18; D12).

use std::time::Duration;

use serde_json::{json, Value};

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

    /// poll.rs's re-handshake: a session lost is re-helloed, never fatal.
    pub async fn hello(&mut self, who: &str) -> Result<(), String> {
        self.session = None;
        let init = json!({
            "jsonrpc": "2.0", "id": self.bump(), "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26", "capabilities": {},
                "clientInfo": {"name": format!("chorux-bridge:{who}"), "version": "0.1.0"}
            }
        });
        if self.post(&init).await?.is_none() {
            return Err("initialize failed (no id-matched response)".into());
        }
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let _ = self.post(&note).await;
        Ok(())
    }

    /// Call a tool. Ok(value) = the tool's content[0].text parsed as JSON
    /// when it parses (sayTo returns a JSON receipt), else the raw text.
    /// Err(reason) = transport failure OR the tool's lawful refusal
    /// (isError results carry the room's verdict text verbatim, e.g.
    /// UNDELIVERABLE — the caller forwards it, never launders it (D9)).
    pub async fn call(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let req = json!({
            "jsonrpc": "2.0", "id": self.bump(), "method": "tools/call",
            "params": {"name": name, "arguments": args}
        });
        let r = self
            .post(&req)
            .await?
            .ok_or_else(|| format!("{name} failed (no id-matched response)"))?;
        let text = r
            .pointer("/result/content/0/text")
            .and_then(|t| t.as_str())
            .ok_or_else(|| format!("{name} failed (no result content[0].text)"))?;
        let is_error = r
            .pointer("/result/isError")
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
        if is_error {
            return Err(text.to_string());
        }
        Ok(serde_json::from_str(text).unwrap_or_else(|_| json!(text)))
    }

    pub async fn join(&mut self, group: &str, handle: &str) -> Result<(), String> {
        self.call("join", json!({"group": group, "handle": handle}))
            .await
            .map(|_| ())
    }

    pub async fn say(
        &mut self,
        group: &str,
        handle: &str,
        text: &str,
        formats: &[&str],
    ) -> Result<(), String> {
        self.call(
            "say",
            json!({"group": group, "handle": handle, "text": text, "formats": formats}),
        )
        .await
        .map(|_| ())
    }

    pub async fn say_to(
        &mut self,
        group: &str,
        handle: &str,
        to: &str,
        text: &str,
    ) -> Result<Value, String> {
        self.call(
            "sayTo",
            json!({"group": group, "handle": handle, "to": to, "text": text}),
        )
        .await
    }
}

/// poll.rs-verbatim in shape: SSE-framed (data: blocks per response) or
/// raw JSON; the id-matched frame is the reply, everything else ignored.
fn find_id_result(raw: &str, payload: &Value) -> Option<Value> {
    let id = payload.get("id")?.as_u64()?;
    let candidates: Vec<&str> = if raw.trim_start().starts_with('{') {
        vec![raw]
    } else {
        raw.lines()
            .filter(|l| l.starts_with("data:"))
            .map(|l| l.trim_start_matches("data:").trim())
            .collect()
    };
    for cand in candidates {
        if let Ok(v) = serde_json::from_str::<Value>(cand) {
            if v.get("id").and_then(|x| x.as_u64()) == Some(id) {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_id_matched_frame_in_sse() {
        let raw = "event:message\ndata:{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\ndata:{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}\n\n";
        let p = json!({"id": 2});
        assert!(find_id_result(raw, &p).is_some());
        let p3 = json!({"id": 3});
        assert!(find_id_result(raw, &p3).is_none());
    }

    #[test]
    fn notification_without_id_yields_none() {
        let raw = "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\n";
        let p = json!({"id": 8});
        assert!(find_id_result(raw, &p).is_none());
    }
}
