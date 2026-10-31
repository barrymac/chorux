package it.coining.chorux.room

import it.coining.chorux.message.ChatEvent
import org.jxmpp.jid.impl.JidCreate
import org.springframework.beans.factory.annotation.Value
import org.springframework.stereotype.Component
import java.security.MessageDigest
import java.nio.file.Path
import java.time.Instant
import java.util.concurrent.ConcurrentHashMap

/** Who is standing in the room. Transport fact only — no roles, no status doctrine. */
data class Occupant(val handle: String, val joinedAt: Instant)

/**
 * The whole answer to "where is everyone at" — read by agents and humans.
 * The room shows; nobody asks anybody anything.
 */
data class Pulse(
    val group: String,
    val occupants: List<Occupant>,
    val recent: List<ChatEvent>,
    val recentThoughts: List<ChatEvent.Thought>,
    /** Distinct recent [[markers]] lifted from says — neurOS-speak surfaces free (L14). */
    val recentLinks: List<String>,
    /** SHA-256 head of the event hash chain — tamper-evident, censorship-resistant by construction. */
    val ledgerHead: String?,
)

/**
 * It's just a chat room. And like any honest chat room it is *content-agnostic*:
 * payloads pass through unmodified, uninterpreted, unpoliced. Meaning lives in
 * clients and harnesses — never here.
 *
 * Reliability shape: bounded history, no fan-out machinery, no blocking under
 * load, state that is explicitly single-node and says so. Everything extra gets
 * refused loudly, never silently absorbed.
 *
 * Observability is events-first and attaches OUTBOARD (L16): the event stream
 * and the hash-chain ledger are the series source — no metrics dialect lives
 * in the room, and logs carry failures only.
 */
class ChatRoom @JvmOverloads constructor(
    val group: String,
    private val historyLimit: Int,
    private val journal: LedgerJournal? = null,
) {
    private val occupants = LinkedHashMap<String, Occupant>()
    private val history = ArrayDeque<ChatEvent>()
    private val thoughts = ArrayDeque<ChatEvent.Thought>()
    private val links = ArrayDeque<String>()

    /**
     * Tamper-evident event chain head. Declared BEFORE the journal-replay init
     * block on purpose: initializers and init blocks run in declaration order,
     * and a later `= null` would wipe a head replayed from the journal.
     */
    private var ledgerHead: String? = null

    init {
        // the persistence tooth: replay the verified journal BEFORE the room
        // takes its first live act — occupants are live facts and never
        // replayed (L19); sealed history and the chain head survive restarts
        journal?.let { j ->
            j.replay().let { rp ->
                rp.events.forEach { e ->
                    history.addLast(e)
                    if (e is ChatEvent.Said) e.links.forEach { links.addLast(it) }
                    if (e is ChatEvent.Thought) thoughts.addLast(e)
                }
                trim(history, historyLimit); trim(thoughts, historyLimit); trim(links, historyLimit)
                ledgerHead = rp.head
            }
        }
    }

    companion object {
        /** Marker grammar: `[[whatever:something]]`. Mechanical lift only (L14). */
        private val WIKI_LINK = Regex("""\[\[([^\[\]]+?)\]\]""")
        fun markupsOf(text: String): List<String> =
            WIKI_LINK.findAll(text).map { it.groupValues[1] }.distinct().toList()
    }

    @Synchronized
    fun join(handle: String): Occupant = occupants.getOrPut(handle) {
        Occupant(handle, Instant.now()).also { record(ChatEvent.Joined(group, handle)) }
    }

    /** Say exactly what [handle] said. The room never edits it, never parses it. */
    @Synchronized
    @JvmOverloads
    fun say(handle: String, text: String, formats: List<String> = emptyList()): ChatEvent.Said {
        requireOccupant(handle)
        return ChatEvent.Said(group, handle, text, formats, markupsOf(text)).also(::record)
    }

    /** Attach a thinking trace. Always optional, always first-class in the pulse. */
    @Synchronized
    fun think(handle: String, trace: String): ChatEvent.Thought {
        requireOccupant(handle)
        return ChatEvent.Thought(group, handle, trace).also { t ->
            record(t); thoughts.addLast(t); trim(thoughts, historyLimit)
        }
    }

    @Synchronized
    fun pulse(): Pulse = Pulse(
        group = group,
        occupants = occupants.values.toList(),
        recent = history.takeLast(50),
        recentThoughts = thoughts.takeLast(25),
        recentLinks = links.takeLast(50).distinct(),
        ledgerHead = ledgerHead,
    )

    private fun requireOccupant(handle: String) =
        require(handle in occupants) { "not in room '$group': $handle — join first (one call, reflexive)" }

    private fun canonicalOf(e: ChatEvent): String {
        val payload = when (e) {
            is ChatEvent.Said -> e.text + "|" + e.formats.joinToString(",") + "|" + e.links.joinToString(",")
            is ChatEvent.Thought -> e.trace
            else -> ""
        }
        return "${e::class.simpleName}|${e.group}|${e.handle}|${e.at}|$payload"
    }

    private fun chainHash(e: ChatEvent): String =
        MessageDigest.getInstance("SHA-256")
            .digest((ledgerHead.orEmpty() + canonicalOf(e)).toByteArray())
            .joinToString("") { "%02x".format(it) }

    private fun record(e: ChatEvent) {
        val head = chainHash(e)
        // append+fsync BEFORE the room mutates: a crash may leave the journal
        // one event ahead of memory (replay heals), never behind (silent loss)
        journal?.append(e, canonicalOf(e), head)
        ledgerHead = head
        history.addLast(e); trim(history, historyLimit)
        if (e is ChatEvent.Said) e.links.forEach { links.addLast(it) }
        trim(links, historyLimit)
    }
    private fun <T> trim(d: ArrayDeque<T>, cap: Int) { while (d.size > cap) d.removeFirst() }
}

/**
 * Rooms are formed reflexively: the first join creates the group. Group
 * formation is cheap on purpose — that is the adoption law, and it dissolves
 * the whole admission-control/reaper era on its own.
 */
@Component
class RoomRegistry(
    @Value("\${chorux.history-limit}") private val historyLimit: Int,
    @Value("\${chorux.ledger.dir:}") private val ledgerDir: String = "",
) {
    private val rooms = ConcurrentHashMap<String, ChatRoom>()

    /** @return the room and whether this call formed it. */
    fun formOrJoin(group: String, handle: String): Pair<ChatRoom, Boolean> {
        requireLegalLocalpart(group)
        val formed = !rooms.containsKey(group)
        val room = rooms.computeIfAbsent(group) {
            ChatRoom(group, historyLimit, journalFor(group))
        }
        if (room.pulse().occupants.none { it.handle == handle }) room.join(handle)
        return room to formed
    }

    /** Empty dir = honest in-memory day one; set = append-only journal, fail loud. */
    private fun journalFor(group: String): LedgerJournal? =
        if (ledgerDir.isBlank()) null else LedgerJournal.open(Path.of(ledgerDir), group)

    fun room(group: String): ChatRoom? = rooms[group]

    /**
     * Zero-mapping guard (docs/07): the group name IS the MUC localpart, so it
     * must survive the rail's own verifier UNCHANGED — refuse at formation,
     * loudly, before the room exists. The rail library is the sole authority:
     * a hand-rolled character set cannot see both failure classes — parse
     * breakage ('/' before '@' yields "localpart can't be null") AND silent
     * case-folding ("Genesis" arrives as "genesis") — and would drift the moment
     * the rail's rules move. Never escape, never fold: identity stays bytes.
     * The "@localhost" jig is a semantically dead scaffold: only the localpart
     * roundtrip is inspected.
     */
    private fun requireLegalLocalpart(group: String) {
        val bare = try {
            JidCreate.entityBareFrom("$group@localhost")
        } catch (e: Exception) {
            val root = generateSequence<Throwable>(e) { it.cause }.last().message
            throw IllegalArgumentException(
                "group name '$group' is not a legal XMPP localpart ($root) — " +
                "zero-mapping law (docs/07): group==localpart exactly; refused at formation, never escaped",
                e
            )
        }
        val roundtrip = bare.getLocalpartOrNull()?.asUnescapedString()
        require(roundtrip == group) {
            "group name '$group' would become '$roundtrip' on the rail — " +
            "zero-mapping law (docs/07): identity stays bytes; refused at formation, never folded"
        }
    }
}
