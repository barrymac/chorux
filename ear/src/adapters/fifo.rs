//! fifo — deliver-fifo's trail, verbatim: the raw event JSON line,
//! nonblocking write, refusal taxonomy (missing / not-a-fifo / no reader /
//! full), every refusal dead-lettered — a refused wake is never a lost wake.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use nix::fcntl::OFlag;
use nix::sys::stat::SFlag;
use serde_json::Value;
use std::os::unix::fs::MetadataExt;

use super::{dead_letter, expand_arg, HarnessAdapter};

pub struct Fifo {
    path: PathBuf,
}

pub fn build(arg: &str) -> Result<Box<dyn HarnessAdapter>, String> {
    Ok(Box::new(Fifo {
        path: expand_arg(arg, "fifo")?,
    }))
}

impl Fifo {
    /// What crosses the wall on this trail: the raw event JSON, one line.
    fn payload(event: &Value) -> String {
        serde_json::to_string(event).unwrap_or_default()
    }
}

impl HarnessAdapter for Fifo {
    fn kind(&self) -> &'static str {
        "fifo"
    }
    fn describe(&self) -> String {
        format!("fifo:{}", self.path.display())
    }
    fn deliver<'a>(
        &'a self,
        event: &'a Value,
        _frame: &'a str,
        dead: &'a Path,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let payload = Self::payload(event);
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

pub(crate) fn fifo_write(path: &Path, payload: &str) -> Result<(), String> {
    let meta = std::fs::metadata(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!(
            "{} does not exist (ear not armed / no reader standing)",
            path.display()
        ),
        _ => format!("cannot stat {}: {e}", path.display()),
    })?;
    if !SFlag::from_bits_truncate(meta.mode()).contains(SFlag::S_IFIFO) {
        return Err(format!("{} is not a fifo", path.display()));
    }
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(OFlag::O_NONBLOCK.bits())
        .open(path)
        .map_err(|e| {
            if e.raw_os_error() == Some(nix::errno::Errno::ENXIO as i32) {
                format!(
                    "{} has no reader (session dead or ear not armed)",
                    path.display()
                )
            } else {
                format!("open {}: {e}", path.display())
            }
        })?;
    // payloads are single lines far below PIPE_BUF: the kernel write is
    // atomic — all of it or WouldBlock, never a torn line
    file.write_all(format!("{payload}\n").as_bytes())
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::WouldBlock => {
                format!("{} full (reader wedged)", path.display())
            }
            _ => format!("write to {} failed: {e}", path.display()),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::sys::stat::Mode as FMode;
    use nix::unistd::mkfifo;

    fn tmp(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let d = std::env::temp_dir().join(format!(
            "chorux-ear-fifo-{}-{name}-{nonce}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn event() -> Value {
        serde_json::json!({"handle":"keel","at":"2026-10-29T14:43:31.028144124Z","text":"ping @omo"})
    }

    #[tokio::test]
    async fn refusal_no_reader_dead_letters() {
        let d = tmp("noref");
        let fifo = d.join("knock.fifo");
        mkfifo(&fifo, FMode::S_IRWXU).unwrap();
        let dead = d.join("dead.jsonl");
        build(fifo.to_str().unwrap())
            .unwrap()
            .deliver(&event(), "frame", &dead)
            .await;
        let dlq = std::fs::read_to_string(&dead).unwrap();
        assert!(dlq.contains("no reader"), "reason missing in: {dlq}");
        // the payload rides the `line` field as a JSON string (quotes escaped) —
        // the contract is the round-trip: parse the record, parse the preserved
        // line, the original event must survive verbatim
        let rec: Value =
            serde_json::from_str(dlq.trim_end()).expect("dead.jsonl is one JSON record");
        let preserved: Value = serde_json::from_str(rec["line"].as_str().expect("line field"))
            .expect("preserved payload parses back as the event");
        assert_eq!(preserved["handle"], "keel");
        assert_eq!(preserved["text"], "ping @omo");
    }

    #[tokio::test]
    async fn refusal_not_a_fifo() {
        let d = tmp("notfifo");
        let plain = d.join("plain");
        std::fs::write(&plain, "x").unwrap();
        let dead = d.join("dead.jsonl");
        build(plain.to_str().unwrap())
            .unwrap()
            .deliver(&event(), "frame", &dead)
            .await;
        let dlq = std::fs::read_to_string(&dead).unwrap();
        assert!(dlq.contains("is not a fifo"), "reason missing in: {dlq}");
    }

    #[tokio::test]
    async fn happy_path_delivers_line() {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        let d = tmp("happy");
        let fifo = d.join("knock.fifo");
        mkfifo(&fifo, FMode::S_IRWXU).unwrap();
        // O_RDONLY|O_NONBLOCK returns immediately with no writer — a reader
        // standing before the write, no timing dependence.
        let mut rfile = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(OFlag::O_NONBLOCK.bits())
            .open(&fifo)
            .unwrap();
        build(fifo.to_str().unwrap())
            .unwrap()
            .deliver(&event(), "frame", &d.join("dead.jsonl"))
            .await;
        let mut line = String::new();
        rfile.read_to_string(&mut line).unwrap();
        assert!(line.starts_with('{') && line.ends_with("}\n"), "line: {line}");
        let parsed: Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(parsed["handle"], "keel");
    }
}
