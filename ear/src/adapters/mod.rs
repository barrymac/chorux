//! adapters — how a wake crosses the wall into one harness.
//!
//! Two families exist (ear/DESIGN.md plug points):
//! - **in-daemon adapters** (this registry): the daemon pokes the harness
//!   from outside — fifo, pi-rpc, hook. Adding a harness = one file
//!   implementing [`HarnessAdapter`] + one line in [`builders`]. No core
//!   edits anywhere else; that is the no-drama bar.
//! - **in-harness extensions** (e.g. paw/chorux-ear.ts): the ear lives
//!   INSIDE the session and calls the harness's own injection API. They
//!   cannot be daemon-side by construction and are not in this registry.
//!
//! Law carried by every adapter: never drop a wake quietly — every refusal
//! dead-letters the exact payload to <state>/dead.jsonl, loud.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

pub mod fifo;
pub mod hook;
pub mod pi_rpc;

pub trait HarnessAdapter: Send + Sync {
    #[allow(dead_code)] // the adapter's self-identity: tests assert it, seat registries will list it
    fn kind(&self) -> &'static str;
    fn describe(&self) -> String;
    /// Deliver one wake: `event` is the raw room event, `frame` the quoted
    /// provenance line, `dead` the dead-letter sink. Every refusal
    /// dead-letters; nothing is dropped quietly.
    fn deliver<'a>(
        &'a self,
        event: &'a Value,
        frame: &'a str,
        dead: &'a Path,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

pub type Builder = fn(&str) -> Result<Box<dyn HarnessAdapter>, String>;

/// The registry. A new harness lands as `("kind", module::build)` here.
pub fn builders() -> &'static [(&'static str, Builder)] {
    &[
        ("fifo", fifo::build),
        ("pi-rpc", pi_rpc::build),
        ("hook", hook::build),
    ]
}

fn kinds() -> String {
    builders()
        .iter()
        .map(|(k, _)| *k)
        .collect::<Vec<_>>()
        .join("|")
}

/// Parse `<kind>:<arg>` (`--inject fifo:...`, config-file inject entries).
pub fn build(spec: &str) -> Result<Box<dyn HarnessAdapter>, String> {
    let (kind, arg) = spec.split_once(':').ok_or_else(|| {
        format!("inject spec '{spec}': want <kind>:<arg> (kinds: {})", kinds())
    })?;
    for (k, b) in builders() {
        if *k == kind {
            return b(arg);
        }
    }
    Err(format!(
        "inject spec '{spec}': unknown adapter '{kind}' (kinds: {})",
        kinds()
    ))
}

pub(crate) async fn dead_letter(dead: &Path, adapter: &str, payload: &str, reason: &str) {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    append_jsonl(
        dead,
        &serde_json::json!({
            "dlq_at_unix_ms": ms,
            "adapter": adapter,
            "reason": reason,
            "line": payload,
        }),
    )
    .await;
    eprintln!(
        "chorux-ear: dead-lettered {} bytes to {}",
        payload.len(),
        dead.display()
    );
}

pub(crate) async fn append_jsonl(f: &Path, v: &Value) {
    // a ~100-byte local append: plain std, no spawn_blocking indirection —
    // the ear's spool must land with OS semantics, nothing in between
    use std::io::Write;
    let res = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(f)
        .and_then(|mut file| file.write_all(format!("{v}\n").as_bytes()));
    if let Err(e) = res {
        // the ear kill condition: never drop a wake — or its dead-letter — quietly
        eprintln!("chorux-ear: spool append failed at {}: {e}", f.display());
    }
}

pub(crate) fn shellexpand(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    p.to_string()
}

pub(crate) fn expand_arg(arg: &str, kind: &str) -> Result<PathBuf, String> {
    if arg.trim().is_empty() {
        return Err(format!("adapter '{kind}': empty argument"));
    }
    Ok(PathBuf::from(shellexpand(arg.trim())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_builds_every_kind_and_refuses_unknown() {
        assert_eq!(build("fifo:/tmp/x.fifo").unwrap().kind(), "fifo");
        assert_eq!(build("pi-rpc:~/x.fifo").unwrap().kind(), "pi-rpc");
        assert_eq!(build("hook:cat > /dev/null").unwrap().kind(), "hook");
        let err = build("smtp:/tmp/x").err().expect("unknown kind must refuse");
        assert!(err.contains("unknown adapter 'smtp'"), "err: {err}");
        let err = build("bogus").err().expect("spec without kind must refuse");
        assert!(err.contains("<kind>:<arg>"), "err: {err}");
    }
}
