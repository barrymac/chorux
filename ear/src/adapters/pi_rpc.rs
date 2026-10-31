//! pi-rpc — deliver-pi-rpc's trail, verbatim: one Pi RPC prompt envelope per
//! wake, steer-shaped (a knock at the next tool boundary, never an
//! interruption), fifo-written with the fifo refusal taxonomy.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use serde_json::Value;

use super::fifo::fifo_write;
use super::{dead_letter, expand_arg, HarnessAdapter};

pub struct PiRpc {
    path: PathBuf,
}

pub fn build(arg: &str) -> Result<Box<dyn HarnessAdapter>, String> {
    Ok(Box::new(PiRpc {
        path: expand_arg(arg, "pi-rpc")?,
    }))
}

impl PiRpc {
    fn envelope(event: &Value) -> String {
        let handle = event.get("handle").and_then(|h| h.as_str()).unwrap_or("?");
        let at = event.get("at").and_then(|a| a.as_str()).unwrap_or("");
        let text = event.get("text").and_then(|t| t.as_str()).unwrap_or("");
        // deliver-pi-rpc's python [11:19] slice on an ASCII ISO timestamp,
        // guarded by byte length
        let hhmmss = if at.len() >= 19 { &at[11..19] } else { "" };
        serde_json::json!({
            "type": "prompt",
            "message": format!("[chorux/{handle} {hhmmss}] {text}"),
            "streamingBehavior": "steer",
        })
        .to_string()
    }
}

impl HarnessAdapter for PiRpc {
    fn kind(&self) -> &'static str {
        "pi-rpc"
    }
    fn describe(&self) -> String {
        format!("pi-rpc:{}", self.path.display())
    }
    fn deliver<'a>(
        &'a self,
        event: &'a Value,
        _frame: &'a str,
        dead: &'a Path,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let payload = Self::envelope(event);
            if let Err(reason) = fifo_write(&self.path, &payload) {
                eprintln!(
                    "chorux-ear: inject {} refused: {reason} — dead-lettering",
                    self.describe()
                );
                dead_letter(dead, &self.describe(), &payload, &reason).await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> Value {
        serde_json::json!({"handle":"keel","at":"2026-10-29T14:43:31.028144124Z","text":"ping @omo"})
    }

    #[test]
    fn envelope_matches_deliver_pi_rpc() {
        let v: Value = serde_json::from_str(&PiRpc::envelope(&event())).unwrap();
        assert_eq!(v["type"], "prompt");
        assert_eq!(v["streamingBehavior"], "steer");
        assert_eq!(v["message"], "[chorux/keel 14:43:31] ping @omo");
    }

    #[test]
    fn envelope_short_at_yields_no_clock() {
        let v: Value = serde_json::from_str(&PiRpc::envelope(&serde_json::json!({
            "handle":"a","at":"short","text":"t"
        })))
        .unwrap();
        assert_eq!(v["message"], "[chorux/a ] t");
    }
}
