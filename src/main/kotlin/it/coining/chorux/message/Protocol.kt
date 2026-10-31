package it.coining.chorux.message

import java.time.Instant

/**
 * The seam: a transport IS the mechanism (chorus + **x**).
 * The room never speaks the wire directly; it emits [ChatEvent]s through this
 * boundary. Day one ships [InMemoryTransport] (single node, honest about it).
 * XMPP/MUC (Smack) is the expected first real mechanism — mechanism, not doctrine.
 */
interface EventTransport {
    fun emit(event: ChatEvent)

    /**
     * The dialogue tier's addressing verb (docs/10): deliver one Said, off-wall,
     * to a single occupant nick — no room event, no tap, no ledger entry. The
     * default refusal IS the lawful verdict: a transport that cannot address
     * occupants fails loud (UNDELIVERABLE), never silently.
     */
    fun sayTo(event: ChatEvent.Said, nick: String) {
        throw UnsupportedOperationException(
            "UNDELIVERABLE: ${this::class.simpleName} cannot address occupants (dialogue tier, docs/10)"
        )
    }
}

/**
 * Law: every occurrence in a chorux room is one of these events.
 * The protocol IS the messages — never endpoints, never hidden channel state.
 * Everything else (command routing, capability teaching) is nudges *inside*
 * ordinary events (docs/01-design-laws.md).
 */
sealed interface ChatEvent {
    val group: String
    val handle: String
    val at: Instant

    data class Joined(override val group: String, override val handle: String,
                      override val at: Instant = Instant.now()) : ChatEvent

    data class Said(override val group: String, override val handle: String,
                    val text: String,
                    /** Declared microformat labels (Atom/RSS extension-element style):
                     *  public, first-class, emergent — any string is valid, invented
                     *  vocabulary is welcome. The room carries them; it never
                     *  interprets what they mean. Meaning belongs to clients. */
                    val formats: List<String> = emptyList(),
                    /** neurOS-speak favor (L14): wiki-link markers `[[x:y]]` are
                     *  lifted *mechanically* out of text at say-time — pure syntax,
                     *  zero ceremony, zero interpretation. They just… show up. */
                    val links: List<String> = emptyList(),
                    override val at: Instant = Instant.now()) : ChatEvent

    /** Reserved: explicit leave. Silently leaving isn't an operation yet (docs/02). */
    data class Left(override val group: String, override val handle: String,
                    override val at: Instant = Instant.now()) : ChatEvent

    /** Thinking trace — a first-class protocol act, not metadata. Transparency of thought. */
    data class Thought(override val group: String, override val handle: String,
                       val trace: String, override val at: Instant = Instant.now()) : ChatEvent
}
