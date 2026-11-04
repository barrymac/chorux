//! hook — the generic escape hatch: a command per wake, the QUOTED room
//! line on stdin, timeout-bounded; failures dead-letter loud.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::process::Command;

use super::{dead_letter, HarnessAdapter};

const HOOK_TIMEOUT: Duration = Duration::from_secs(15);

pub struct Hook {
    cmd: String,
}

pub fn build(arg: &str) -> Result<Box<dyn HarnessAdapter>, String> {
    if arg.trim().is_empty() {
        return Err("adapter 'hook': empty command".into());
    }
    Ok(Box::new(Hook {
        cmd: arg.trim().to_string(),
    }))
}

impl Hook {
    async fn run(&self, quoted: &str) -> Result<(), String> {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(&self.cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let mut w = BufWriter::new(&mut stdin);
            w.write_all(quoted.as_bytes())
                .await
                .map_err(|e| format!("stdin: {e}"))?;
            w.flush().await.map_err(|e| format!("stdin: {e}"))?;
        }
        let status = tokio::time::timeout(HOOK_TIMEOUT, child.wait())
            .await
            .map_err(|_| format!("hook exceeded {HOOK_TIMEOUT:?}"))?
            .map_err(|e| format!("wait: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("exit {status}"))
        }
    }
}

impl HarnessAdapter for Hook {
    fn kind(&self) -> &'static str {
        "hook"
    }
    fn describe(&self) -> String {
        format!("hook:{}", self.cmd)
    }
    fn deliver<'a>(
        &'a self,
        _event: &'a Value,
        frame: &'a str,
        dead: &'a Path,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if let Err(e) = self.run(frame).await {
                eprintln!("chorux-ear: hook failed: {e} — dead-lettering");
                dead_letter(dead, &self.describe(), frame, &e).await;
            }
        })
    }
}
