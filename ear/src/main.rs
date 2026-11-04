//! chorux-ear — the poke daemon and multi-seat agent client (ear/DESIGN.md).
//!
//! ONE daemon per host seats MANY agents (DESIGN law 1): each seat holds a
//! resilient connection to a chorux room — the SSE tap (default) or the
//! poll tier absorbed from paw/pi-ear.py — gates on wake tokens (@own-handle,
//! @broadcast, extras; a `--session` seat answers @nick/<session8> exactly),
//! and pokes the local agent with the room line QUOTED
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
    /// Session seat (card 5e8a1c42): first 8 chars of the MCP session id —
    /// `None` = a lane seat. A session seat answers `@nick/<session8>`
    /// exactly and never the bare lane handle (docs/09 exact-token law).
    session: Option<String>,
    state: PathBuf,
    also: Vec<String>,
    source: Source,
    poll_every: u64,
    injects: Vec<Box<dyn adapters::HarnessAdapter>>,
}

impl Config {
    /// docs/09: the seat's wake handle — a lane seat answers its bare nick;
    /// a session seat answers `<nick>/<session8>` (the bare lane stays with
    /// the lane seat, for lane-wide ops).
    pub(crate) fn handle(&self) -> String {
        match &self.session {
            Some(tok) => format!("{}/{}", self.nick, tok),
            None => self.nick.clone(),
        }
    }
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
    session: Option<String>,
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
           --session ID   seat a session, not the lane: the seat answers exactly\n\
                           @NICK/<first 8 of ID> and never the bare @NICK (docs/09\n\
                           exact-token law); delivery cursors key per seat, so a\n\
                           restart resumes this seat's stream, never a sibling's\n\
           --config FILE  multi-seat registry: JSON {{\"seats\":[{{base,room,nick,session,state,also,hook,source,poll_every,inject}}]}}\n\
                          (one daemon, many seats — DESIGN law 1; state defaults to\n\
                          ~/.local/state/chorux-ear/<room>-<seat-slug> per seat; a\n\
                          lane seat's slug is its nick, a session seat's carries\n\
                          its token)\n\
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
    let session = match &s.session {
        Some(x) => Some(session_token(x).map_err(|e| format!("{ctx}: {e}"))?),
        None => None,
    };
    let nick = s.nick.to_lowercase();
    let handle = match &session {
        Some(tok) => format!("{nick}/{tok}"),
        None => nick.clone(),
    };
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
                .join(format!("{}-{}", s.room, slug(&handle)))
        }
    };
    Ok(Config {
        base: s.base.trim_end_matches('/').to_string(),
        room: s.room.clone(),
        nick,
        session,
        state,
        also: s.also.iter().map(|t| t.to_lowercase()).collect(),
        source,
        poll_every: s.poll_every.unwrap_or(5),
        injects,
    })
}

fn parse_cli_seat(args: &mut Arguments) -> Config {
    let invite: Option<String> = want(args.opt_value_from_str("--invite"));
    let (base, room, nick, session, mut also) = match invite {
        Some(blob) => parse_invite(&blob),
        None => {
            let b: String = need(args.opt_value_from_str("--base"));
            let r: String = need(args.opt_value_from_str("--room"));
            let n: String = need(args.opt_value_from_str("--nick"));
            let s: Option<String> = want(args.opt_value_from_str("--session"));
            (b, r, n, s, Vec::new())
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
    let session = session.map(|s| die_or(session_token(&s)));
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
        session,
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
/// the rest are room=, nick=, session= (session seat) and also= (extra
/// wake tokens). Unparseable or incomplete → usage, loud, never a guessed seat.
fn parse_invite(blob: &str) -> (String, String, String, Option<String>, Vec<String>) {
    let mut parts = blob.split(';').map(str::trim).filter(|p| !p.is_empty());
    let base = parts.next().unwrap_or_else(|| usage()).to_string();
    let mut room = String::new();
    let mut nick = String::new();
    let mut session = None;
    let mut also = Vec::new();
    for part in parts {
        let (k, v) = part.split_once('=').unwrap_or_else(|| usage());
        match k.to_lowercase().as_str() {
            "room" => room = v.trim().to_string(),
            "nick" => nick = v.trim().to_string(),
            "session" => session = Some(v.trim().to_string()),
            "also" => also.push(v.trim().to_lowercase()),
            _ => usage(),
        }
    }
    if room.is_empty() || nick.is_empty() {
        usage();
    }
    (base, room, nick, session, also)
}

async fn run_seat(cfg: Config) -> Result<(), String> {
    let handle = cfg.handle();
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
    lock.write_all(format!("pid={} room={} handle @{}\n", std::process::id(), cfg.room, handle).as_bytes())
        .map_err(|e| format!("seat lock write: {e}"))?;

    // card 5e8a1c42: the delivery cursor keys per seat — two seats in one
    // state dir never resume each other's stream, and a restart of THIS
    // seat resumes losslessly (a legacy un-keyed cursor seeds the keyed
    // file once, so an upgraded seat keeps its place, zero replay).
    let id_name = sse_id_file(&cfg.room, &handle);
    migrate_cursor(&cfg.state, &id_name, &format!("last-id-{}", cfg.room));
    let id_file = cfg.state.join(&id_name);
    let spool = cfg.state.join("spool.jsonl");
    let dead = cfg.state.join("dead.jsonl");

    if cfg.source == Source::Poll {
        eprintln!(
            "chorux-ear: seat @{} polling {} in {} as @{}, @broadcast, @all{}{}",
            handle,
            cfg.base,
            cfg.room,
            handle,
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
        handle,
        handle,
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

/// docs/09's gate, made exact (card 5e8a1c42): `@handle` addresses ONE seat
/// as ONE token — the char run after `@` is matched whole, so `@lane/a2`
/// never wakes `lane/a`, `@omosome` never wakes `omo`, and a session seat
/// (`nick/<session8>`) does not answer the bare lane handle. `@all` and
/// `@broadcast` stay the reserved all-calls. Non-amplification: a line never
/// rings the speaker's own bell.
pub(crate) fn wakes(text: &str, handle: &str, nick: &str, also: &[String], from: &str) -> bool {
    if from.eq_ignore_ascii_case(handle) || from.eq_ignore_ascii_case(nick) {
        return false;
    }
    let mut hit = false;
    for token in mention_tokens(text) {
        if token == "all" || token == "broadcast" {
            return true; // reserved all-calls — never a seat name
        }
        if token == handle || also.iter().any(|t| token == t.trim_start_matches('@')) {
            hit = true;
        }
    }
    hit
}

/// The @-mention tokens of a line: each `@` runs to the first char outside
/// the handle grammar `[A-Za-z0-9_/-]`, lowercased — `@lane/a,` yields
/// `lane/a`, `@OMO.` yields `omo`.
fn mention_tokens(text: &str) -> Vec<String> {
    let low = text.to_lowercase();
    let mut out: Vec<String> = Vec::new();
    let mut rest = low.as_str();
    while let Some(i) = rest.find('@') {
        let tail = &rest[i + 1..];
        let end = tail
            .char_indices()
            .find(|(_, c)| !(c.is_alphanumeric() || matches!(c, '-' | '_' | '/')))
            .map_or(tail.len(), |(e, _)| e);
        if end > 0 {
            out.push(tail[..end].to_string());
        }
        rest = &tail[end..];
    }
    out
}

/// The session seat's token (card 5e8a1c42): MCP session ids are long; the
/// seat is addressed by its first 8 chars — unique per live connection,
/// short enough to say aloud. Loud refusal on anything that would break the
/// @-token grammar: a seat that cannot be addressed exactly is no seat.
fn session_token(session: &str) -> Result<String, String> {
    let tok: String = session.trim().to_lowercase().chars().take(8).collect();
    if tok.is_empty() {
        return Err(format!(
            "session '{session}' is empty — a session seat needs a session id to seed its token"
        ));
    }
    if !tok
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/'))
    {
        return Err(format!(
            "session '{session}': first 8 chars '{tok}' are not a legal wake token ([a-z0-9_/-]) — the seat would be unaddressable, refusing loud"
        ));
    }
    Ok(tok)
}

/// Filename slug for one seat key: [A-Za-z0-9._-] pass through, everything
/// else percent-encodes — `lane/a` and `lane-a` can never collide on disk.
pub(crate) fn slug(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Per-seat delivery cursor (card 5e8a1c42): persisted per (group, session) —
/// the seat's handle keys it; two seats sharing a state dir can never resume
/// each other's stream, and a seat restart resumes losslessly.
pub(crate) fn sse_id_file(room: &str, handle: &str) -> String {
    format!("last-id-{room}@{}", slug(handle))
}

/// The poll tier's cursor, keyed the same way.
pub(crate) fn poll_cursor_file(room: &str, handle: &str) -> String {
    format!("cursor-{room}@{}", slug(handle))
}

/// Zero-replay upgrade (card 5e8a1c42): a seat that ran before per-seat
/// cursor keying keeps its place in the stream — the legacy cursor is read
/// once into the keyed file; nothing replays from scratch.
pub(crate) fn migrate_cursor(state: &std::path::Path, keyed: &str, legacy: &str) {
    let keyed = state.join(keyed);
    if keyed.exists() {
        return;
    }
    match std::fs::read_to_string(state.join(legacy)) {
        Ok(pos) => {
            if let Err(e) = std::fs::write(&keyed, pos) {
                eprintln!(
                    "chorux-ear: cursor seed {} -> {}: {e} — first connect replays the ring once",
                    legacy,
                    keyed.display()
                );
            }
        }
        Err(_) => {} // no legacy cursor: first boot, the ring catches up
    }
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

    let handle = cfg.handle();
    if !wakes(text, &handle, &cfg.nick, &cfg.also, from) {
        return;
    }

    // provenance in, instructions out — the framed line is what the agent reads
    let frame = framed(from, text);
    eprintln!("chorux-ear: WAKE — {frame}");
    adapters::append_jsonl(
        spool,
        &serde_json::json!({
            "id": id, "from": from, "at": at, "wake": format!("@{handle}"),
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
        assert!(wakes("hey @omo ping", "omo", "omo", &also, "keel"));
        assert!(wakes("hey @OMO ping", "omo", "omo", &also, "keel"));
        assert!(wakes("all hands @broadcast", "omo", "omo", &also, "keel"));
        assert!(wakes("ping @all", "omo", "omo", &also, "keel"));
        assert!(wakes("for the @team", "omo", "omo", &also, "keel"));
        assert!(!wakes("for the team (no at)", "omo", "omo", &also, "keel"));
        assert!(!wakes("hey @keel only", "omo", "omo", &also, "keel"));
        assert!(!wakes("plain line", "omo", "omo", &also, "keel"));
    }

    /// P2 (card 5e8a1c42): exact-token addressing — prefix neighbors never wake.
    #[test]
    fn wake_gate_is_exact_token_prefix_neighbors_never_wake() {
        let none: Vec<String> = Vec::new();
        // a session seat answers ONLY its full token
        assert!(wakes("@lane/a go", "lane/a", "lane", &none, "keel"));
        assert!(!wakes("@lane/a2 go", "lane/a", "lane", &none, "keel"));
        assert!(!wakes("@lane/aa go", "lane/a", "lane", &none, "keel"));
        assert!(!wakes("@lane go", "lane/a", "lane", &none, "keel"));
        // a lane seat answers the bare nick, not its lane's session tokens
        assert!(wakes("@lane go", "lane", "lane", &none, "keel"));
        assert!(!wakes("@lane/a go", "lane", "lane", &none, "keel"));
        // no substring leaks in either direction
        assert!(!wakes("@omosome hi", "omo", "omo", &none, "keel"));
        assert!(wakes("@OMO.", "omo", "omo", &none, "keel"));
        assert!(!wakes("@broadcasting news", "omo", "omo", &none, "keel"));
        // a session seat's also-tokens gate exactly as well
        let also = vec!["team".to_string()];
        assert!(wakes("@team go", "lane/a", "lane", &also, "keel"));
        assert!(!wakes("@team-x go", "lane/a", "lane", &also, "keel"));
    }

    /// docs/09 non-amplification: your own line never rings your own bell.
    #[test]
    fn wake_gate_skips_own_lines() {
        let none: Vec<String> = Vec::new();
        assert!(!wakes("@all hands", "omo", "omo", &none, "omo"));
        assert!(!wakes("@omo do it", "omo", "omo", &none, "omo"));
        assert!(!wakes("@lane/a run", "lane/a", "lane", &none, "lane"));
        // someone else's line still rings
        assert!(wakes("@omo do it", "omo", "omo", &none, "keel"));
    }

    #[test]
    fn mention_tokens_split_on_the_handle_grammar() {
        assert_eq!(
            mention_tokens("hey @lane/a, ping @OMO. and @all"),
            vec!["lane/a".to_string(), "omo".to_string(), "all".to_string()]
        );
        assert!(mention_tokens("no tokens here").is_empty());
        assert_eq!(mention_tokens("@@weird"), vec!["weird".to_string()]);
    }

    #[test]
    fn slug_and_cursor_names_key_the_seat_exactly() {
        assert_eq!(slug("lane/a"), "lane%2Fa");
        assert_ne!(slug("lane/a"), slug("lane-a"));
        assert_eq!(sse_id_file("ee", "lane/a"), "last-id-ee@lane%2Fa");
        assert_eq!(poll_cursor_file("ee", "lane/a"), "cursor-ee@lane%2Fa");
        assert_ne!(sse_id_file("ee", "lane/a"), sse_id_file("ee", "lane-a"));
        // lane seats keep their legacy names inside their legacy dirs
        assert_eq!(sse_id_file("ee", "omo"), "last-id-ee@omo");
    }

    /// P3 (card 5e8a1c42): a restart never replays what the seat already
    /// heard — the cursor survives, keyed per seat; a legacy un-keyed cursor
    /// seeds the keyed file once, so an upgraded seat keeps its place.
    #[test]
    fn cursor_keys_per_seat_and_migrates_legacy_once() {
        let dir = std::env::temp_dir().join(format!("chorux-ear-cursor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // fresh seat: no cursor anywhere — nothing seeded, first boot catches up
        migrate_cursor(&dir, "last-id-ee@lane%2Fa", "last-id-ee");
        assert!(!dir.join("last-id-ee@lane%2Fa").exists());

        // legacy cursor present: seeded into the keyed file, once
        std::fs::write(dir.join("last-id-ee"), "42").unwrap();
        migrate_cursor(&dir, "last-id-ee@lane%2Fa", "last-id-ee");
        assert_eq!(std::fs::read_to_string(dir.join("last-id-ee@lane%2Fa")).unwrap(), "42");

        // a later run never regresses the keyed cursor to the stale legacy one
        std::fs::write(dir.join("last-id-ee"), "7").unwrap();
        migrate_cursor(&dir, "last-id-ee@lane%2Fa", "last-id-ee");
        assert_eq!(std::fs::read_to_string(dir.join("last-id-ee@lane%2Fa")).unwrap(), "42");

        // a second seat keys its own place, never the first seat's
        std::fs::write(dir.join("last-id-ee"), "99").unwrap();
        migrate_cursor(&dir, "last-id-ee@lane%2Fb", "last-id-ee");
        assert_eq!(std::fs::read_to_string(dir.join("last-id-ee@lane%2Fb")).unwrap(), "99");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_seats_get_their_own_handle_and_state() {
        let raw = r#"{"seats":[
            {"base":"http://a:8080","room":"ee","nick":"Lane","session":"AB12CD34-9999-x"},
            {"base":"http://a:8080","room":"ee","nick":"lane"},
            {"base":"http://a:8080","room":"ee","nick":"x","session":"!!!"}
        ]}"#;
        let file: SeatsFile = serde_json::from_str(raw).unwrap();
        let a = seat_from_json(0, &file.seats[0]).unwrap();
        assert_eq!(a.handle(), "lane/ab12cd34");
        assert!(a.state.to_string_lossy().ends_with("ee-lane%2Fab12cd34"));
        // lane seats keep their legacy default dir and bare handle
        let b = seat_from_json(1, &file.seats[1]).unwrap();
        assert_eq!(b.handle(), "lane");
        assert!(b.state.to_string_lossy().ends_with("ee-lane"));
        // an unaddressable session refuses loud, never guesses a seat
        let Err(err) = seat_from_json(2, &file.seats[2]) else {
            panic!("seat #2 with an illegal session token must refuse, never construct a seat");
        };
        assert!(err.contains("legal wake token"), "err: {err}");
        assert!(err.contains("seat #2"), "err: {err}");
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
        let (base, room, nick, session, also) =
            parse_invite("http://h:8080; room=chorux ; nick=Omo ; also=Team");
        assert_eq!(base, "http://h:8080");
        assert_eq!(room, "chorux");
        assert_eq!(nick, "Omo");
        assert_eq!(session, None);
        assert_eq!(also, vec!["team".to_string()]);

        let (_, _, _, session, _) =
            parse_invite("http://h:8080;room=ee;nick=lane;session=AB12CD34-9999");
        assert_eq!(session.as_deref(), Some("AB12CD34-9999"));
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
            session: None,
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
