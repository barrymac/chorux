//! chorux-bridge — the I3 duplex bridge (bridge/DESIGN.md).
//!
//! Inbound only: stock-client humans in watched MUC rooms -> chorux room
//! ingress (D5); MUC-PM to this nick -> sayTo control channel with the
//! verdict returned as a PM (D8/D9). Outbound onto the rail stays the
//! daemon's mirror (L16) — this process writes nothing to room walls.
//!
//! Config: BRIDGE_* env only, every missing or blank value named in one
//! startup error, never the first only (D10). Trust: BRIDGE_TRUST must
//! be exactly `system` in v0 — anything else refuses startup (D11);
//! private-CA rails enter via the container's trust store, never via an
//! insecure mode (there is no insecure mode).

mod map;
mod mcp;

use std::collections::HashSet;
use std::env;
use std::process::exit;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::time::sleep;
use tokio_xmpp::connect::DnsConfig;
use tokio_xmpp::minidom::Element;
use tokio_xmpp::parsers::message::MessageType;
use tokio_xmpp::parsers::stanza::Stanza;
use tokio_xmpp::xmlstream::Timeouts;
use tokio_xmpp::{Client, Event};

const USAGE: &str = "usage: chorux-bridge\n  env: BRIDGE_MCP_BASE BRIDGE_XMPP_JID BRIDGE_XMPP_PASSWORD \\\n       BRIDGE_XMPP_HOST BRIDGE_XMPP_PORT BRIDGE_XMPP_MUC_DOMAIN \\\n       BRIDGE_XMPP_NICK BRIDGE_ROOMS BRIDGE_ALLOWED_JIDS \\\n       [BRIDGE_MIRROR_NICK] [BRIDGE_HUMAN_PREFIX] BRIDGE_TRUST=system";

struct Config {
    mcp_base: String,
    xmpp_jid: String,
    xmpp_password: String,
    xmpp_host: String,
    xmpp_port: u16,
    muc_domain: String,
    nick: String,
    mirror_nick: Option<String>,
    rooms: HashSet<String>,
    allowed: HashSet<String>,
    human_prefix: String,
}

fn load_config() -> Result<Config, String> {
    let mut missing: Vec<String> = Vec::new();
    let mut want = |k: &str| -> String {
        match env::var(k) {
            Ok(v) if !v.trim().is_empty() => v,
            _ => {
                missing.push(k.to_string());
                String::new()
            }
        }
    };
    let mcp_base = want("BRIDGE_MCP_BASE");
    let xmpp_jid = want("BRIDGE_XMPP_JID");
    let xmpp_password = want("BRIDGE_XMPP_PASSWORD");
    let xmpp_host = want("BRIDGE_XMPP_HOST");
    let xmpp_port_raw = want("BRIDGE_XMPP_PORT");
    let muc_domain = want("BRIDGE_XMPP_MUC_DOMAIN");
    let nick = want("BRIDGE_XMPP_NICK");
    let rooms_raw = want("BRIDGE_ROOMS");
    let allowed_raw = want("BRIDGE_ALLOWED_JIDS");
    let trust = env::var("BRIDGE_TRUST").unwrap_or_default();
    if trust.trim() != "system" {
        missing.push(
            "BRIDGE_TRUST (must be exactly `system` in v0; the pinned-CA connector is designed (D11) but UNWRITTEN — refuse, never degrade)"
                .to_string(),
        );
    }
    if !missing.is_empty() {
        return Err(format!(
            "chorux-bridge: missing/invalid config values:\n  {}\n\n{USAGE}",
            missing.join("\n  ")
        ));
    }
    let xmpp_port: u16 = xmpp_port_raw
        .trim()
        .parse()
        .map_err(|e| format!("chorux-bridge: BRIDGE_XMPP_PORT not a port ({e})\n\n{USAGE}"))?;
    let rooms: HashSet<String> = rooms_raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if rooms.is_empty() {
        return Err(format!("chorux-bridge: BRIDGE_ROOMS has no usable room name\n\n{USAGE}"));
    }
    let allowed: HashSet<String> = allowed_raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if allowed.is_empty() {
        return Err(format!(
            "chorux-bridge: BRIDGE_ALLOWED_JIDS empty — the trust envelope may not be empty (D4)\n\n{USAGE}"
        ));
    }
    let mirror_nick = env::var("BRIDGE_MIRROR_NICK")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(m) = &mirror_nick {
        if *m == nick {
            return Err(format!(
                "chorux-bridge: BRIDGE_XMPP_NICK == BRIDGE_MIRROR_NICK is a routing hole (D2): control PMs must land on the bridge\n\n{USAGE}"
            ));
        }
    }
    let human_prefix = env::var("BRIDGE_HUMAN_PREFIX")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "human".to_string());
    Ok(Config {
        mcp_base,
        xmpp_jid,
        xmpp_password,
        xmpp_host,
        xmpp_port,
        muc_domain,
        nick,
        mirror_nick,
        rooms,
        allowed,
        human_prefix,
    })
}

/// Strip a possibly-full jid to its bare form.
fn bare(jid: &str) -> String {
    jid.split('/').next().unwrap_or(jid).to_string()
}

/// Element for an occupant-bound presence (XEP-0045 §join).
fn join_presence(room_jid: &str) -> Element {
    Element::builder("presence", "jabber:client")
        .attr(rxml::xml_ncname!("to").to_owned(), room_jid)
        .append(Element::builder("x", "http://jabber.org/protocol/muc").build())
        .build()
}

/// Element for a MUC-PM (chat message addressed to room@domain/nick).
fn pm_element(to: &str, body: &str) -> Element {
    let mut msg = Element::builder("message", "jabber:client")
        .attr(rxml::xml_ncname!("type").to_owned(), "chat")
        .attr(rxml::xml_ncname!("to").to_owned(), to)
        .build();
    msg.append_child(Element::builder("body", "jabber:client").append(body).build());
    msg
}

fn stanza_of(el: Element, what: &str) -> Result<Stanza, String> {
    Stanza::try_from(el).map_err(|e| format!("lower {what}: {e}"))
}

struct Bridge {
    cfg: Config,
    mcp: mcp::McpClient,
    joined: HashSet<String>, // "room\0handle" pairs already joined this run
    ingress_failed: HashSet<String>, // failure-transition memory (D9)
}

impl Bridge {
    /// On the room half: ensure the MCP session, ensure the seat, say.
    async fn ensure_join(&mut self, room: &str, handle: &str) -> Result<(), String> {
        let key = format!("{room}\0{handle}");
        if self.joined.contains(&key) {
            return Ok(());
        }
        self.mcp.join(room, handle).await?;
        self.joined.insert(key);
        eprintln!("[bridge] seat held: {handle} in {room}");
        Ok(())
    }

    /// D3..D7 + D5/D9: one live groupchat line -> one lawful say, or a
    /// loud, classified drop. Returns after exactly one of: say minted,
    /// line dropped (logged), error surfaced to the human (PM, D9).
    async fn ingress(&mut self, client: &mut Client, from: &str, el: &Element, body: &str) {
        let room = bare(from).split('@').next().unwrap_or("").to_string();
        if !self.cfg.rooms.contains(&room) {
            return; // not ours; ignore silently (watched set is explicit)
        }
        if map::has_delay(el) {
            eprintln!("[bridge] {room}: history dropped ({body:.40}…)");
            return;
        }
        if map::looks_like_envelope(body) {
            eprintln!("[bridge] {room}: mirror traffic dropped (envelope shape)");
            return;
        }
        // Sender attribution is the whole trust envelope (D3/D4).
        let sender_full = match map::muc_user_jid(el) {
            Some(j) => j,
            None => {
                eprintln!(
                    "[bridge] {room}: FAIL-CLOSED drop — no muc#user item; room must be non-anonymous (D3)"
                );
                return;
            }
        };
        let sender_bare = bare(&sender_full);
        if !self.cfg.allowed.contains(&sender_bare) {
            eprintln!("[bridge] {room}: drop from non-listed {sender_bare} (D4)");
            return;
        }
        let handle = map::mint_handle(&self.cfg.human_prefix, &sender_bare);
        let key = format!("{room}\0{sender_bare}");
        let result = async {
            self.ensure_join(&room, &handle).await?;
            self.mcp
                .say(&room, &handle, body, &["i3#bridge-ingress"])
                .await
        }
        .await;
        match result {
            Ok(()) => {
                if self.ingress_failed.remove(&key) {
                    eprintln!("[bridge] {room}: ingress recovered for {sender_bare}");
                }
            }
            Err(e) => {
                if self.ingress_failed.insert(key) {
                    eprintln!("[bridge] {room}: ingress failure for {sender_bare}: {e}");
                    let pm = pm_element(
                        from,
                        &format!("ingress broken right now: {e} — your line did not reach the room (D9)"),
                    );
                    if let Ok(st) = stanza_of(pm, "pm") {
                        let _ = client.send_stanza(st).await;
                    }
                }
            }
        }
    }

    /// D8/D9: one MUC-PM -> one sayTo + the verdict PM, or a refusal PM.
    async fn control(&mut self, client: &mut Client, from: &str, el: &Element, body: &str) {
        let room = bare(from).split('@').next().unwrap_or("").to_string();
        if !self.cfg.rooms.contains(&room) {
            return;
        }
        if map::has_delay(el) {
            eprintln!("[bridge] {room}: control history dropped");
            return;
        }
        let sender_full = match map::muc_user_jid(el) {
            Some(j) => j,
            None => {
                eprintln!("[bridge] {room}: FAIL-CLOSED control drop — no muc#user item (D3)");
                let pm = pm_element(from, "cannot verify who you are (room reports no identity) — refusing (D3)");
                if let Ok(st) = stanza_of(pm, "pm") {
                    let _ = client.send_stanza(st).await;
                }
                return;
            }
        };
        let sender_bare = bare(&sender_full);
        if !self.cfg.allowed.contains(&sender_bare) {
            eprintln!("[bridge] {room}: control drop from non-listed {sender_bare} (D4)");
            return; // silence toward the unlisted; never a hint of the grammar
        }
        let handle = map::mint_handle(&self.cfg.human_prefix, &sender_bare);
        let reply = match map::parse_control(body) {
            None => map::control_grammar_hint().to_string(),
            Some((to, text)) => {
                let res = async {
                    self.ensure_join(&room, &handle).await?;
                    self.mcp.say_to(&room, &handle, &to, &text).await
                }
                .await;
                match res {
                    Ok(v) => {
                        let via = v
                            .get("deliveredVia")
                            .and_then(|x| x.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|i| i.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                            .unwrap_or_else(|| "transport".to_string());
                        map::verdict_delivered(&to, &room, &via)
                    }
                    Err(e) => map::verdict_refused(&to, &room, &e),
                }
            }
        };
        let pm = pm_element(from, &reply);
        if let Ok(st) = stanza_of(pm, "pm") {
            if let Err(e) = client.send_stanza(st).await {
                eprintln!("[bridge] {room}: verdict PM send failed: {e}");
            }
        }
    }

    async fn run(&mut self) -> Result<(), String> {
        let cfg = &self.cfg;
        let jid: tokio_xmpp::jid::Jid = cfg
            .xmpp_jid
            .parse()
            .map_err(|e| format!("chorux-bridge: BRIDGE_XMPP_JID invalid: {e}"))?;
        let dns = DnsConfig::addr(&format!("{}:{}", cfg.xmpp_host, cfg.xmpp_port));
        let mut client = Client::new_starttls(jid, cfg.xmpp_password.clone(), dns, Timeouts::default());
        // The room half must be alive before we mint a single line (D10).
        self.mcp.hello(&cfg.nick).await?;
        eprintln!(
            "[bridge] mcp session up for {}; rooms: {:?}",
            cfg.nick, self.cfg.rooms
        );
        loop {
            match client.next().await {
                Some(Event::Online { .. }) => {
                    eprintln!("[bridge] rail online as {}", self.cfg.nick);
                    let rooms: Vec<String> = self.cfg.rooms.iter().cloned().collect();
                    for room in rooms {
                        let to = format!("{room}@{}/{}", self.cfg.muc_domain, self.cfg.nick);
                        match stanza_of(join_presence(&to), "presence") {
                            Ok(st) => {
                                if let Err(e) = client.send_stanza(st).await {
                                    eprintln!("[bridge] join {room} send failed: {e}");
                                } else {
                                    eprintln!("[bridge] joined {to}");
                                }
                            }
                            Err(e) => eprintln!("[bridge] join {room} build failed: {e}"),
                        }
                    }
                }
                Some(Event::Stanza(Stanza::Message(msg))) => {
                    let from = msg.from.clone().map(|j| j.to_string()).unwrap_or_default();
                    let body = msg
                        .bodies
                        .values()
                        .next()
                        .map(|b| b.clone())
                        .unwrap_or_default();
                    if body.trim().is_empty() {
                        continue;
                    }
                    // An Element view for the guard walkers (D3/D6).
                    let el: Option<Element> = Element::try_from(msg.clone()).ok();
                    match msg.type_ {
                        MessageType::Groupchat => {
                            if let Some(e) = el {
                                self.ingress(&mut client, &from, &e, &body).await;
                            }
                        }
                        MessageType::Chat | MessageType::Normal => {
                            if let Some(e) = el {
                                self.control(&mut client, &from, &e, &body).await;
                            }
                        }
                        _ => {}
                    }
                }
                Some(Event::Stanza(Stanza::Presence(p))) => {
                    eprintln!(
                        "[bridge] rail presence (FAIL-LOUD, was silently discarded in v0): {p:?}"
                    );
                }
                Some(_) => {}
                None => {
                    eprintln!("[bridge] stream ended — the autoreconnect driver exits with us");
                    return Err("stream ended".to_string());
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    match load_config() {
        Err(e) => {
            eprintln!("{e}");
            exit(2);
        }
        Ok(cfg) => {
            let mcp = mcp::McpClient::new(&cfg.mcp_base);
            let mut bridge = Bridge {
                cfg,
                mcp,
                joined: HashSet::new(),
                ingress_failed: HashSet::new(),
            };
            loop {
                if let Err(e) = bridge.run().await {
                    eprintln!("[bridge] run failed: {e} — owned retry budget: redial in 5s");
                    bridge.joined.clear();
                    bridge.ingress_failed.clear();
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}
