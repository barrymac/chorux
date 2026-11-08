//! map.rs — the pure half of I3: every decision the bridge makes that can
//! be tested without a wire lives here as a function over values (D3..D9).
//! main.rs owns side effects; this file owns judgment.

use serde_json::Value;
use tokio_xmpp::minidom::Element;

/// D5 — the seat the bridge holds for a rail account:
/// `<prefix>-<localpart-of-bare-jid>`. The prefix brands the seat as
/// bridge-held so a room's occupants stay honest about who multiplexed
/// the speaker in.
pub fn mint_handle(prefix: &str, sender_bare_jid: &str) -> String {
    let local = sender_bare_jid
        .split('@')
        .next()
        .unwrap_or(sender_bare_jid);
    format!("{prefix}-{local}")
}

/// D8 — control grammar: `@handle the text…`. Leading whitespace-free
/// handle token, then at least one space and a non-empty body.
/// Returns None for everything else (the caller PMs the grammar back).
pub fn parse_control(body: &str) -> Option<(String, String)> {
    let t = body.trim_start();
    let rest = t.strip_prefix('@')?;
    let (handle, text) = rest.split_once(char::is_whitespace)?;
    let handle = handle.trim();
    let text = text.trim();
    if handle.is_empty() || text.is_empty() {
        return None;
    }
    Some((handle.to_string(), text.to_string()))
}

/// D6 — a stanza is HISTORY iff it carries a delay element: either the
/// modern urn:xmpp:delay (XEP-0203) or the legacy jabber:x:delay
/// (XEP-0091, belt-and-braces against servers that still emit it).
pub fn has_delay(el: &Element) -> bool {
    fn walk(e: &Element) -> bool {
        let is_delay = (e.name() == "delay" && e.ns() == "urn:xmpp:delay")
            || (e.name() == "x" && e.ns() == "jabber:x:delay");
        is_delay || e.children().any(walk)
    }
    walk(el)
}

/// D3 — the sender's real JID from the muc#user payload (XEP-0045):
/// `<x xmlns='http://jabber.org/protocol/muc#user'><item jid='…'/></x>`.
/// Absent = fail-closed (None); the room must be non-anonymous.
pub fn muc_user_jid(el: &Element) -> Option<String> {
    el.children()
        .find(|c| c.name() == "x" && c.ns() == "http://jabber.org/protocol/muc#user")
        .and_then(|x| x.children().find(|i| i.name() == "item"))
        .and_then(|i| i.attr("jid").map(|s| s.to_string()))
}

/// D7 — mirror-emission shape: a JSON object whose `event` is a known
/// wire-envelope kind AND that carries a string handle. Human chat text
/// never takes this shape by accident; a human who types it on purpose
/// reads as machine traffic by design (D7c).
pub fn looks_like_envelope(body: &str) -> bool {
    let v: Value = match serde_json::from_str(body.trim_start()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let event_known = matches!(
        v.get("event").and_then(|e| e.as_str()),
        Some("Said") | Some("Thought") | Some("Joined")
    );
    event_known && v.get("handle").is_some_and(|h| h.is_string())
}

/// D9 — the verdict PM text, one shape for success and failure alike:
/// the human reads where their line went (or why it didn't).
pub fn verdict_delivered(to: &str, room: &str, via: &str) -> String {
    format!("delivered to @{to} in {room} ({via})")
}

pub fn verdict_refused(to: &str, room: &str, reason: &str) -> String {
    format!("refused: @{to} in {room}: {reason}")
}

/// D8 refusal text for anything that isn't the grammar.
pub fn control_grammar_hint() -> &'static str {
    "control grammar: @<handle> <text> — a MUC private message to this nick reaches the occupant's seat; room lines are ingress by D5"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_handle_uses_localpart_of_the_bare() {
        assert_eq!(mint_handle("human", "barry@example.org"), "human-barry");
        assert_eq!(mint_handle("human", "op-7@example.org"), "human-op-7");
        // input is a BARE jid by contract (main strips resources before
        // calling); a stray full jid still mints its localpart, never panics
        assert_eq!(
            mint_handle("human", "op-7@example.org/workstation"),
            "human-op-7"
        );
    }

    #[test]
    fn mint_handle_handles_bare_without_at() {
        assert_eq!(mint_handle("human", "guest"), "human-guest");
    }

    #[test]
    fn control_parses_handle_and_text() {
        assert_eq!(
            parse_control("@k3-07 what did you ship thursday?"),
            Some(("k3-07".to_string(), "what did you ship thursday?".to_string()))
        );
    }

    #[test]
    fn control_refuses_the_degenerates() {
        assert_eq!(parse_control("no at sign"), None);
        assert_eq!(parse_control("@lonely"), None);
        assert_eq!(parse_control("@  two handles"), None);
        assert_eq!(parse_control(""), None);
        assert_eq!(parse_control("@handle   "), None);
    }

    #[test]
    fn delay_detection_modern_and_legacy() {
        let live: Element = "<message xmlns='jabber:client' type='groupchat'><body>hello</body></message>"
            .parse()
            .unwrap();
        assert!(!has_delay(&live));
        let hist: Element = "<message xmlns='jabber:client' type='groupchat'><delay xmlns='urn:xmpp:delay' stamp='2026-10-31T00:00:00Z'/><body>old</body></message>"
            .parse()
            .unwrap();
        assert!(has_delay(&hist));
        let legacy: Element = "<message xmlns='jabber:client' type='groupchat'><x xmlns='jabber:x:delay' stamp='20261031T00:00:00'/><body>old</body></message>"
            .parse()
            .unwrap();
        assert!(has_delay(&legacy));
        let nested: Element = "<message xmlns='jabber:client'><x xmlns='http://jabber.org/protocol/muc#user'><delay xmlns='urn:xmpp:delay'/></x></message>"
            .parse()
            .unwrap();
        assert!(has_delay(&nested));
    }

    #[test]
    fn muc_user_jid_reads_item_jid() {
        let el: Element = "<message xmlns='jabber:client' type='groupchat'><x xmlns='http://jabber.org/protocol/muc#user'><item affiliation='member' jid='barry@example.org/phone' role='participant'/></x><body>hi</body></message>"
            .parse()
            .unwrap();
        assert_eq!(
            muc_user_jid(&el),
            Some("barry@example.org/phone".to_string())
        );
    }

    #[test]
    fn muc_user_jid_fail_closed_when_absent() {
        let el: Element =
            "<message xmlns='jabber:client' type='groupchat'><body>hi</body></message>"
                .parse()
                .unwrap();
        assert_eq!(muc_user_jid(&el), None);
       let semi: Element = "<message xmlns='jabber:client' type='groupchat'><x xmlns='http://jabber.org/protocol/muc#user'/><body>hi</body></message>"
            .parse()
            .unwrap();
        assert_eq!(muc_user_jid(&semi), None);
    }

    #[test]
    fn envelope_shape_catches_mirror_traffic() {
        assert!(looks_like_envelope(
            "{\"event\":\"Said\",\"group\":\"g\",\"handle\":\"h\",\"at\":\"t\",\"text\":\"x\"}"
        ));
        assert!(looks_like_envelope(
            "{\"event\":\"Thought\",\"group\":\"g\",\"handle\":\"h\",\"at\":\"t\",\"trace\":\"x\"}"
        ));
        assert!(!looks_like_envelope("just a chat line"));
        assert!(!looks_like_envelope("{\"event\":\"Said\"}"));
        assert!(!looks_like_envelope("{\"handle\":\"h\",\"event\":\"MadeUp\"}"));
        assert!(!looks_like_envelope("[1,2,3]"));
    }

    #[test]
    fn verdict_texts_read_as_receipts() {
        assert_eq!(
            verdict_delivered("k3-07", "ee-swarm", "XmppEventTransport"),
            "delivered to @k3-07 in ee-swarm (XmppEventTransport)"
        );
        assert!(verdict_refused("ghost", "ee-swarm", "UNDELIVERABLE").contains("UNDELIVERABLE"));
    }
}
