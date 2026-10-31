//! chorux-ear — the poke daemon and multi-seat agent client (ear/DESIGN.md).
//!
//! ONE daemon per host seats MANY agents (DESIGN law 1): each seat holds a
//! resilient connection to a chorux room — the SSE tap (default) or the
//! poll tier absorbed from paw/pi-ear.py — gates on wake tokens (@own-nick,
//! @broadcast, extras), and pokes the local agent with the room line QUOTED
//! (provenance in, instructions out: the room is read, it never holds the
//! pen) via the spool, a --hook, and/or the fifo injectors absorbed from
//! paw/deliver-fifo + paw/deliver-pi-rpc.
//!
//! Resilience law: reconnect after dropouts — exponential backoff with cap,
//! a stale-stream watchdog (no bytes ⇒ dead ⇒ reconnect), and persisted
//! Last-Event-ID so even a process restart resumes losslessly while the
//! server's replay ring still remembers. A `:gap` from the server is logged
//! loud and lands in the spool — silence is never replayed as completeness.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use fs2::FileExt;
use futures_util::StreamExt;
use pico_args::Arguments;

mod adapters;
mod poll;

const WATCHDOG: Duration = Duration::from_secs(90);
const BACKOFF_INIT: Duration = Duration::from_secs(1);
const BACKOFF_CAP: Duration = Duration::from_secs(30);
const STABLE_AFTER: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    Sse,
    Poll,
}

fn parse_source(s: &str) -> Result<Source, String> {
    match s {
        "sse" => Ok(Source::Sse),
        "poll" => Ok(Source::Poll),
        "xmpp" => Err(
            "source 'xmpp': the XMPP seat source is not staffed yet — the daemon-ward \
             fork rides it next (ear/DESIGN.md plug points); refusing loud, not guessing \
             a transport"
                .into(),
        ),
        other => Err(format!("source '{other}': want sse|poll")),
    }
}

struct Config {
    base: String,
    room: String,
    nick: String,
    state: PathBuf,
    also: Vec<String>,
    source: Source,
    poll_every: u64,
    injects: Vec<Box<dyn adapters::HarnessAdapter>>,
}

#[derive(serde::Deserialize)]
struct SeatsFile {
    seats: Vec<SeatJson>,
}

#[derive(serde::Deserialize)]
struct SeatJson {
    base: String,
    room: String,
    nick: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    also: Vec<String>,
    #[serde(default)]
    hook: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    poll_every: Option<u64>,
    #[serde(default, rename = "inject")]
    injects: Vec<String>,
}

fn usage() -> ! {
    eprintln!(
        "chorux-ear — an ear for a sleeping agent (chorux room -> gated quoted poke)\n\
         \n\
         usage: chorux-ear --base URL --room NAME --nick NICK [options]\n\
                chorux-ear --config SEATS.json\n\
         \n\
           --base URL     chorux server, e.g. http://host:8080\n\
           --room NAME    the room to listen in\n\
           --nick NICK    your handle in the room; lines mentioning @NICK wake you\n\
           --config FILE  multi-seat registry: JSON {{\"seats\":[{{base,room,nick,state,also,hook,source,poll_every,inject}}]}}\n\
                          (one daemon, many seats — DESIGN law 1; state defaults to\n\
                          ~/.local/state/chorux-ear/<room>-<nick> per seat)\n\
           --state DIR    state dir (default: ~/.local/state/chorux-ear)\n\
           --also TOKEN   extra wake token (repeatable; @broadcast and @all always wake)\n\
           --source WHAT  sse (default; the stream tap) or poll (pi-ear's tier:\n\
                          request/response MCP only — tunnels, five-volt hardware)\n\
           --poll-every S poll cadence seconds (default 5; poll source only)\n\
           --hook CMD     legacy sugar for --inject hook:CMD — a command per wake,\n\
           --inject SPEC  last mile, repeatable: <kind>:<arg>. Kinds today:\n\
                          fifo:<path> (raw event JSON line, deliver-fifo's\n\
                          refusal law), pi-rpc:<path> (deliver-pi-rpc steer\n\
                          envelopes), hook:<cmd> — a new harness is one adapter\n\
                          file + one registry line (ear/src/adapters/)\n\
         \n\
         every wake ALSO appends one JSON line to <state>/spool.jsonl;\n\
         adapter refusals are dead-lettered loud to <state>/dead.jsonl —\n\
         never silently dropped. wake up: read the spool, or let your runner's own\n\
         injection ride --inject."
    );
    std::process::exit(2)
}

fn need<T>(r: Result<Option<T>, pico_args::Error>) -> T {
    r.ok().flatten().unwrap_or_else(|| usage())
}

fn want<T>(r: Result<Option<T>, pico_args::Error>) -> Option<T> {
    r.ok().flatten()
}

fn parse_args() -> Vec<Config> {
    let mut args = Arguments::from_env();
    let config_path: Option<PathBuf> = want(args.opt_value_from_str("--config"));
    if let Some(p) = config_path {
        return parse_seats_file(&p).unwrap_or_else(|e| {
            eprintln!("chorux-ear: config {}: {e}", p.display());
            std::process::exit(1);
        });
    }
    vec![parse_cli_seat(&mut args)]
}

fn parse_seats_file(p: &PathBuf) -> Result<Vec<Config>, String> {
    let raw = std::fs::read_to_string(p).map_err(|e| format!("read: {e}"))?;
    let file: SeatsFile = serde_json::from_str(&raw).map_err(|e| format!("parse: {e}"))?;
    if file.seats.is_empty() {
        return Err(format!(
            "no seats in {} — an empty registry seats nobody",
            p.display()
        ));
    }
    file.seats
        .iter()
        .enumerate()
        .map(|(i, s)| seat_from_json(i, s))
        .collect()
}

fn seat_from_json(idx: usize, s: &SeatJson) -> Result<Config, String> {
    let ctx = format!("seat #{idx} (nick {})", s.nick);
    let source =
        parse_source(s.source.as_deref().unwrap_or("sse")).map_err(|e| format!("{ctx}: {e}"))?;
    let mut injects = Vec::new();
    for spec in &s.injects {
        injects.push(adapters::build(spec).map_err(|e| format!("{ctx}: {e}"))?);
    }
    if let Some(cmd) = &s.hook {
        injects.push(
            adapters::build(&format!("hook:{cmd}")).map_err(|e| format!("{ctx}: {e}"))?,
        );
    }
    let state = match &s.state {
        Some(p) => PathBuf::from(adapters::shellexpand(p)),
        None => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home)
                .join(".local/state/chorux-ear")
                .join(format!("{}-{}", s.room, s.nick))
        }
    };
    Ok(Config {
        base: s.base.trim_end_matches('/').to_string(),
        room: s.room.clone(),
        nick: s.nick.to_lowercase(),
        state,
        also: s.also.iter().map(|t| t.to_lowercase()).collect(),
        source,
        poll_every: s.poll_every.unwrap_or(5),
        injects,
    })
}

fn parse_cli_seat(args: &mut Arguments) -> Config {
    let invite: Option<String> = want(args.opt_value_from_str("--invite"));
    let (base, room, nick, mut also) = match invite {
        Some(blob) => parse_invite(&blob),
        None => {
            let b: String = need(args.opt_value_from_str("--base"));
            let r: String = need(args.opt_value_from_str("--room"));
            let n: String = need(args.opt_value_from_str("--nick"));
            (b, r, n, Vec::new())
        }
    };
    let state: Option<PathBuf> = want(args.opt_value_from_str("--state"));
    while let Some(t) = want(args.opt_value_from_str::<_, String>("--also")) {
        also.push(t.to_lowercase());
    }
    let hook: Option<String> = want(args.opt_value_from_str("--hook"));
    let source_str: Option<String> = want(args.opt_value_from_str("--source"));
    let poll_every: Option<u64> = want(args.opt_value_from_str("--poll-every"));

    let source = die_or(parse_source(source_str.as_deref().unwrap_or("sse")));
    let mut injects = Vec::new();
    while let Some(spec) = want(args.opt_value_from_str::<_, String>("--inject")) {
        injects.push(die_or(adapters::build(&spec)));
    }
    if let Some(cmd) = hook {
        injects.push(die_or(adapters::build(&format!("hook:{cmd}"))));
    }
    let state = state.unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".local/state/chorux-ear")
    });
    Config {
        base: base.trim_end_matches('/').to_string(),
        room,
        nick: nick.to_lowercase(),
        state,
        also,
        source,
        poll_every: poll_every.unwrap_or(5),
        injects,
    }
}

fn die_or<T>(r: Result<T, String>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => {
            eprintln!("chorux-ear: {e}");
            std::process::exit(2);
        }
    }
}

#[tokio::main]
async fn main() {
    let seats = parse_args();
    if seats.len() == 1 {
        let cfg = seats.into_iter().next().expect("one seat");
        if let Err(e) = run_seat(cfg).await {
            eprintln!("chorux-ear: fatal: {e}");
            std::process::exit(1);
        }
        return;
    }
    eprintln!(
        "chorux-ear: {} seats, one daemon (DESIGN law 1) — python retired, the daemon is the client",
        seats.len()
    );
    let mut handles = Vec::new();
    for cfg in seats {
        handles.push(tokio::spawn(async move {
            if let Err(e) = run_seat(cfg).await {
                eprintln!("chorux-ear: seat fatal: {e}");
            }
        }));
    }
    for h in handles {
        let _ = h.await;
    }
}

/// The dog tier (ear/DESIGN.md): ONE paste carries the whole seat.
/// Grammar: `<base>;<k=v>...` — first field is the daemon base URL,
/// the rest are room= and nick= (also= for extra wake tokens).
/// Unparseable or incomplete → usage, loud, never a guessed seat.
fn parse_invite(blob: &str) -> (String, String, String, Vec<String>) {
    let mut parts = blob.split(';').map(str::trim).filter(|p| !p.is_empty());
    let base = parts.next().unwrap_or_else(|| usage()).to_string();
    let mut room = String::new();
    let mut nick = String::new();
    let mut also = Vec::new();
    for part in parts {
        let (k, v) = part.split_once('=').unwrap_or_else(|| usage());
        match k.to_lowercase().as_str() {
            "room" => room = v.trim().to_string(),
            "nick" => nick = v.trim().to_string(),
            "also" => also.push(v.trim().to_lowercase()),
            _ => usage(),
        }
    }
    if room.is_empty() || nick.is_empty() {
        usage();
    }
    (base, room, nick, also)
}

async fn run_seat(cfg: Config) -> Result<(), String> {
    std::fs::create_dir_all(&cfg.state)
        .map_err(|e| format!("state dir {}: {e}", cfg.state.display()))?;

    // ONE seat per state dir, enforced loudly: two ears on one dir
    // double-deliver wakes (measured 2026-10-29) — the second seat
    // refuses, names the holder, and exits; a wake is never silently
    // split between two pokers.
    let seat = cfg.state.join("seat.lock");
    let mut lock = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&seat)
        .map_err(|e| format!("seat lock {}: {e}", seat.display()))?;
    if lock.try_lock_exclusive().is_err() {
        let holder = std::fs::read_to_string(&seat).unwrap_or_else(|_| "unreadable".into());
        return Err(format!(
            "state dir {} already seats an ear — seat.lock held [{holder}] — refusing to double-deliver; this seat is taken",
            cfg.state.display()
        ));
    }
    lock.set_len(0).map_err(|e| format!("seat lock write: {e}"))?;
    lock.write_all(format!("pid={} room={}\n", std::process::id(), cfg.room).as_bytes())
        .map_err(|e| format!("seat lock write: {e}"))?;

    let id_file = cfg.state.join(format!("last-id-{}", cfg.room));
    let spool = cfg.state.join("spool.jsonl");
    let dead = cfg.state.join("dead.jsonl");

    if cfg.source == Source::Poll {
        eprintln!(
            "chorux-ear: seat @{} polling {} in {} as @{}, @broadcast, @all{}{}",
            cfg.nick,
            cfg.base,
            cfg.room,
            cfg.nick,
            if cfg.also.is_empty() {
                String::new()
            } else {
                format!(", {}", cfg.also.join(", "))
            },
            cfg.injects
                .iter()
                .map(|i| format!(" — inject {}", i.describe()))
                .collect::<String>()
        );
        poll::poll_loop(&cfg, &spool, &dead).await;
        return Ok(());
    }

    let url = format!("{}/rooms/{}/events.stream", cfg.base, cfg.room);
    let mut backoff = BACKOFF_INIT;

    eprintln!(
        "chorux-ear: listening {} as @{} (wake on @{}, @broadcast, @all{}){}",
        url,
        cfg.nick,
        cfg.nick,
        if cfg.also.is_empty() {
            String::new()
        } else {
            format!(", {}", cfg.also.join(", "))
        },
        cfg.injects
            .iter()
            .map(|i| format!(" — inject {}", i.describe()))
            .collect::<String>()
    );

    loop {
        let connected_at = Instant::now();
        match listen_once(&cfg, &url, &id_file, &spool, &dead).await {
            Ok(()) => eprintln!("chorux-ear: stream ended clean — reconnecting"),
            Err(e) => {
                if e.starts_with("no room") {
                    // absent room is a WAIT, not a wound: groups form on join,
                    // and servers restart — the ear out-waits both, loudly
                    eprintln!("chorux-ear: {e} — waiting for it to form (join it, or check the name)");
                } else {
                    eprintln!("chorux-ear: stream dropped: {e} — reconnecting in {backoff:?}");
                }
            }
        }
        if connected_at.elapsed() > STABLE_AFTER {
            backoff = BACKOFF_INIT;
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(BACKOFF_CAP);
    }
}

pub(crate) fn wakes(text: &str, nick: &str, also: &[String]) -> bool {
    let low = text.to_lowercase();
    low.contains(&format!("@{nick}"))
        || low.contains("@broadcast")
        || low.contains("@all")
        || also.iter().any(|t| low.contains(t.as_str()))
}

pub(crate) fn framed(from: &str, text: &str) -> String {
    format!("room line from {from}: {text}")
}

async fn listen_once(
    cfg: &Config,
    url: &str,
    id_file: &PathBuf,
    spool: &PathBuf,
    dead: &PathBuf,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let mut req = client.get(url).header("accept", "text/event-stream");
    match read_id(id_file).await {
        Some(id) => req = req.header("last-event-id", id.to_string()),
        None => {}
    }

    let resp = client
        .execute(req.build().map_err(|e| e.to_string())?)
        .await
        .map_err(|e| format!("connect: {e}"))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(format!("no room '{}'", cfg.room));
    }
    if !resp.status().is_success() {
        return Err(format!("http {}", resp.status()));
    }
    eprintln!("chorux-ear: connected");

    let mut stream = resp.bytes_stream();
    let mut line: Vec<u8> = Vec::with_capacity(512);
    let mut data: Vec<String> = Vec::new();
    let mut ev_id: Option<u64> = None;
    let mut ev_name = String::new();
    let mut gap = false;

    loop {
        let chunk = tokio::time::timeout(WATCHDOG, stream.next())
            .await
            .map_err(|_| format!("watchdog: no bytes for {WATCHDOG:?}"))?;
        let chunk = match chunk {
            None => return Ok(()),
            Some(Err(e)) => return Err(format!("read: {e}")),
            Some(Ok(b)) => b,
        };

        for &byte in chunk.iter() {
            if byte != b'\n' {
                line.push(byte);
                continue;
            }
            let text = String::from_utf8_lossy(&line).trim_end_matches('\r').to_string();
            line.clear();

            if text.is_empty() {
                // blank line = frame dispatch
                if gap {
                    eprintln!("chorux-ear: GAP — server rang the honesty bell; history scrolled. catch up via pulse.");
                    adapters::append_jsonl(
                        spool,
                        &serde_json::json!({"gap": true, "note": "history scrolled past Last-Event-ID; catch up via pulse"}),
                    )
                    .await;
                    gap = false;
                } else if !data.is_empty() {
                    dispatch(cfg, &ev_name, &ev_id, &data.join("\n"), spool, dead).await;
                    if let Some(id) = ev_id {
                        write_id(id_file, id).await;
                    }
                }
                data.clear();
                ev_id = None;
                ev_name.clear();
                continue;
            }

            if let Some(comment) = text.strip_prefix(':') {
                if comment.trim() == "gap" {
                    gap = true;
                } // ":hb" and friends: warmth only
            } else if let Some(d) = text.strip_prefix("data:") {
                data.push(d.trim_start().to_string());
            } else if let Some(i) = text.strip_prefix("id:") {
                ev_id = i.trim().parse().ok();
            } else if let Some(n) = text.strip_prefix("event:") {
                ev_name = n.trim().to_string();
            }
        }
    }
}

/// The gate and the poke, law 4: the room line is delivered QUOTED.
async fn dispatch(
    cfg: &Config,
    name: &str,
    id: &Option<u64>,
    data: &str,
    spool: &PathBuf,
    dead: &PathBuf,
) {
    if name != "Said" {
        return; // joins/leaves/thoughts: visible in pulse, not wake-worthy by default
    }
    let v: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return, // unparseable frame is not ours to interpret — skip quiet
    };
    let text = v.get("text").and_then(|t| t.as_str()).unwrap_or("");
    let from = v.get("handle").and_then(|t| t.as_str()).unwrap_or("?");
    let at = v.get("at").and_then(|t| t.as_str()).unwrap_or("?");

    if !wakes(text, &cfg.nick, &cfg.also) {
        return;
    }

    // provenance in, instructions out — the framed line is what the agent reads
    let frame = framed(from, text);
    eprintln!("chorux-ear: WAKE — {frame}");
    adapters::append_jsonl(
        spool,
        &serde_json::json!({
            "id": id, "from": from, "at": at, "wake": format!("@{}", cfg.nick),
            "text": text, "framed": frame, "source": "sse",
        }),
    )
    .await;
    for adapter in &cfg.injects {
        adapter.deliver(&v, &frame, dead).await;
    }
}

async fn read_id(f: &PathBuf) -> Option<u64> {
    tokio::fs::read_to_string(f)
        .await
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

async fn write_id(f: &PathBuf, id: u64) {
    let _ = tokio::fs::write(f, id.to_string()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_gate_matches_own_nick_broadcast_all_and_extras() {
        let also = vec!["team".to_string()];
        assert!(wakes("hey @omo ping", "omo", &also));
        assert!(wakes("hey @OMO ping", "omo", &also));
        assert!(wakes("all hands @broadcast", "omo", &also));
        assert!(wakes("ping @all", "omo", &also));
        assert!(wakes("for the @team", "omo", &also));
        assert!(!wakes("hey @keel only", "omo", &also));
        assert!(!wakes("plain line", "omo", &also));
    }

    #[test]
    fn framed_line_carries_provenance() {
        assert_eq!(
            framed("keel", "ping @omo"),
            "room line from keel: ping @omo"
        );
    }

    #[test]
    fn source_xmpp_refuses_loud_not_guessing() {
        let err = parse_source("xmpp").unwrap_err();
        assert!(err.contains("not staffed"), "err: {err}");
        assert!(parse_source("sse").unwrap() == Source::Sse);
        assert!(parse_source("poll").unwrap() == Source::Poll);
        assert!(parse_source("carrier-pigeon").is_err());
    }

    #[test]
    fn invite_paste_carries_the_whole_seat() {
        let (base, room, nick, also) =
            parse_invite("http://h:8080; room=chorux ; nick=Omo ; also=Team");
        assert_eq!(base, "http://h:8080");
        assert_eq!(room, "chorux");
        assert_eq!(nick, "Omo");
        assert_eq!(also, vec!["team".to_string()]);
    }

    #[test]
    fn seats_file_parses_and_defaults() {
        let raw = r#"{"seats":[
            {"base":"http://a:8080/","room":"chorux","nick":"Omo","also":["Team"],
             "inject":["pi-rpc:~/k.fifo","hook:cat > /dev/null"]},
            {"base":"http://b:8080","room":"fleet","nick":"keel","source":"poll","poll_every":11}
        ]}"#;
        let file: SeatsFile = serde_json::from_str(raw).unwrap();
        assert_eq!(file.seats.len(), 2);
        let a = seat_from_json(0, &file.seats[0]).unwrap();
        assert_eq!(a.base, "http://a:8080");
        assert_eq!(a.nick, "omo");
        assert_eq!(a.source, Source::Sse);
        assert_eq!(a.injects.len(), 2);
        assert_eq!(a.injects[0].kind(), "pi-rpc");
        assert_eq!(a.injects[1].kind(), "hook");
        let b = seat_from_json(1, &file.seats[1]).unwrap();
        assert_eq!(b.source, Source::Poll);
        assert_eq!(b.poll_every, 11);
        assert!(b.injects.is_empty());
    }

    #[test]
    fn seat_error_names_the_seat() {
        let s = SeatJson {
            base: "http://a:8080".into(),
            room: "chorux".into(),
            nick: "x".into(),
            state: None,
            also: vec![],
            hook: None,
            source: Some("xmpp".into()),
            poll_every: None,
            injects: vec![],
        };
        let err = seat_from_json(0, &s).err().expect("xmpp seat must refuse");
        assert!(err.contains("seat #0"), "err: {err}");
        assert!(err.contains("not staffed"), "err: {err}");
    }
}
