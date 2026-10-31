package it.coining.chorux.surface

import it.coining.chorux.message.ChatEvent
import it.coining.chorux.message.EventTransport
import it.coining.chorux.room.Occupant
import it.coining.chorux.room.RoomRegistry
import org.springframework.ai.tool.annotation.Tool
import org.springframework.ai.tool.annotation.ToolParam
import org.springframework.stereotype.Service

/**
 * The tools surface — three tools by design, four by decree: `sayTo` exists
 * only through docs/10's written case against L8 (the dialogue tier).
 *
 * The service is non-opinionated about *content*: it does not parse, route,
 * interpret or police payloads. `nudges` only ever point at mechanism that
 * already exists — progressive discovery happens inside the room, not in docs.
 */

data class JoinResult(
    val group: String,
    val handle: String,
    val formed: Boolean,
    val occupants: List<Occupant>,
    val nudges: List<String>,
)

data class SayResult(
    val said: ChatEvent.Said,
    val nudges: List<String>,
)

/** docs/10: the off-wall direct reply's receipt — who carried it, never a room event. */
data class SayToResult(
    val to: String,
    val text: String,
    val deliveredVia: List<String>,
    val nudges: List<String>,
)

data class PulseResult(
    val group: String,
    val occupants: List<Occupant>,
    val recent: List<ChatEvent>,
    val recentThoughts: List<ChatEvent.Thought>,
    /** Distinct recent [[markers]] — neurOS-speak surfacing, free (docs/01 L14). */
    val recentLinks: List<String>,
    /** Tamper-evident chain head — see docs/02. */
    val ledgerHead: String?,
    val nudges: List<String>,
)

@Service
class ChatRoomTools(
    private val registry: RoomRegistry,
    /** The seam taps (docs/07): emission happens HERE — the member's intentional act (L9), never the room's. */
    private val transports: List<EventTransport> = emptyList(),
) {

    @Tool(description = "Stand in a chorux chat group. If the group does not exist it is formed reflexively — arrival IS formation. Handles are claimed, not owned; say whatever you came to say.")
    fun join(
        @ToolParam(description = "the group name — any name; absent groups form on arrival") group: String,
        @ToolParam(description = "your handle in this group") handle: String,
    ): JoinResult {
        val (room, formed) = registry.formOrJoin(group, handle)
        transports.forEach { it.emit(ChatEvent.Joined(group, handle)) }
        val occs = room.pulse().occupants
        return JoinResult(
            group, handle, formed, occs,
            nudges = buildList {
                if (formed) add("This room formed because you arrived. Anyone with the surface can be next — no gates exist.")
                if (occs.size > 1) add("Others stand here: ${occs.take(12).joinToString { it.handle }} — `pulse` reads the room without asking anyone anything.")
            },
        )
    }

    @Tool(description = "Speak into a group you stand in. STRICT CONTRACT: the call schema is exactly the listed parameters — calls carrying extra/unknown fields are rejected whole BEFORE the room hears them, and a rejected say is a silently lost message. Send only what is listed. Text passes through unmodified — chorux is a messaging system; it never tells you what to say. Declare message-layer microformats in `formats` (any strings — emergent vocabulary welcome); the room carries them publicly, without interpreting them. Optionally attach thinkingTrace; traces are part of the pulse. Wake convention (client-side, never parsed here): '@handle' addresses a member, '@all'/'@broadcast' address everyone — put them in `text` only.")
    fun say(
        @ToolParam(description = "the group") group: String,
        @ToolParam(description = "your handle") handle: String,
        @ToolParam(description = "what you say — opaque to the room") text: String,
        @ToolParam(description = "declared microformat labels for this say (emergent; e.g. 'link#atom', 'claim#work-graph')", required = false) formats: List<String>? = null,
        @ToolParam(description = "optional thinking trace attached to this say", required = false) thinkingTrace: String? = null,
    ): SayResult {
        val room = registry.room(group) ?: error("no such group: $group — join first (one call, reflexive)")
        val said = room.say(handle, text, formats?.distinct() ?: emptyList())
        transports.forEach { it.emit(said) }
        if (!thinkingTrace.isNullOrBlank()) transports.forEach { it.emit(room.think(handle, thinkingTrace)) }
        return SayResult(
            said,
            nudges = buildList {
                add("`pulse` shows the room's current mind — recent says incl. declared formats, plus any thinking traces.")
                if (said.links.isNotEmpty())
                    add("neurOS-speak recognized: ${said.links.size} marker(s) lifted and surfaced in the pulse — that favor is free, always (L14).")
            },
        )
    }

    @Tool(description = "Direct reply, off the room wall (dialogue tier, docs/10): reaches ONE occupant's seat in this group and mints NO room event — the journal never sees it. The exchange's witness is the ack hop: your addressee acks wall-text '@<your handle> + probe-id', and that ack is what the journal holds. STRICT CONTRACT: the call schema is exactly the listed parameters — calls carrying extra/unknown fields are rejected whole BEFORE anything is sent, and a rejected sayTo is a silently lost reply. The addressee is an occupant nick in this group. Text passes through unmodified. If no transport can address occupants the call fails UNDELIVERABLE — the lawful verdict, never laundered; the interim form is wall-text '@nick + probe-id', classified SEAL-1-journal honestly.")
    fun sayTo(
        @ToolParam(description = "the group") group: String,
        @ToolParam(description = "your handle") handle: String,
        @ToolParam(description = "the addressee's occupant nick in this group") to: String,
        @ToolParam(description = "what you say — opaque, carried off-wall") text: String,
    ): SayToResult {
        registry.room(group) ?: error("no such group: $group — join first (one call, reflexive)")
        // wire-shaped only (docs/10): never recorded in the room, never emitted to a tap
        val said = ChatEvent.Said(group, handle, text)
        val delivered = mutableListOf<String>()
        val refused = mutableListOf<String>()
        transports.forEach { t ->
            try {
                t.sayTo(said, to)
                delivered.add(t::class.simpleName ?: t.toString())
            } catch (e: Exception) {
                refused.add("${t::class.simpleName ?: t}: ${e.message}")
            }
        }
        if (delivered.isEmpty())
            error("UNDELIVERABLE: no transport can address occupants in $group — ${refused.joinToString("; ")} — lawful verdict (docs/10); interim form is wall-text '@nick + probe-id' (SEAL-1-journal)")
        return SayToResult(
            to, text, delivered,
            nudges = listOf("the witness for this off-wall reply is the ack hop: your addressee acks wall-text '@$handle + probe-id', and that ack is the journal's record"),
        )
    }

    @Tool(description = "The group's pulse: who stands here, recent events, recent thinking traces. Substitute for polling each other — the room's whole reason to exist.")
    fun pulse(@ToolParam(description = "the group") group: String): PulseResult {
        val room = registry.room(group) ?: error("no such group: $group — join first (one call, reflexive)")
        val p = room.pulse()
        return PulseResult(
            p.group, p.occupants, p.recent, p.recentThoughts, p.recentLinks, p.ledgerHead,
            nudges = emptyList(),
        )
    }
}
