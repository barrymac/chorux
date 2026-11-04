package it.coining.chorux.room

import tools.jackson.databind.ObjectMapper
import it.coining.chorux.message.ChatEvent
import java.nio.ByteBuffer
import java.nio.channels.FileChannel
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardOpenOption
import java.security.MessageDigest

/**
 * The append-only ledger journal — the persistence tooth (2026-10-30, by
 * operator order; mortality exhibit: a restart wiped the room nine times in
 * one night and the first customer day paid the price).
 *
 * Laws this file keeps:
 * - **Append+fsync BEFORE the room mutates.** A crash may leave the journal
 *   one event ahead of memory — replay heals that. Never the reverse: an
 *   event the room remembers but the journal lacks is a silent loss, the one
 *   defect class this codebase refuses everywhere.
 * - **One JSON line per sealed event**: the docs/02 wire shape plus the two
 *   chain values (`canonical`, `head`). The canonical form itself does not
 *   move (docs/02 is unchanged); the journal stores it beside the event so
 *   replay verifies the chain without re-deriving payload rules.
 * - **Replay verifies, never trusts**: each line's head must equal
 *   SHA-256(prevHead + canonical) — a mismatch anywhere is tamper-evidence
 *   and the room refuses to form, naming the file and line.
 * - **A torn tail is a crash artifact, not tamper**: an unparseable or
 *   unterminated FINAL line is truncated loudly; an unparseable MIDDLE line
 *   refuses the boot.
 * - **Occupants are live facts, never replayed** (L19): presence re-forms by
 *   real joins; only the sealed history and the head survive.
 * - **Fail loud on every IO path** (L11): durability was configured — a room
 *   that cannot persist refuses to pretend otherwise.
 */
class LedgerJournal private constructor(
    private val file: Path,
    private val mapper: ObjectMapper,
) {
    private val channel: FileChannel

    init {
        try {
            channel = FileChannel.open(file, StandardOpenOption.CREATE, StandardOpenOption.APPEND)
        } catch (e: Exception) {
            throw IllegalStateException(
                "ledger journal $file: cannot open for append (${e.message}) — " +
                    "durability is configured, refusing silent in-memory fallback (L11 fail-loud)",
                e
            )
        }
    }

    companion object {
        fun open(dir: Path, group: String): LedgerJournal {
            try {
                Files.createDirectories(dir)
            } catch (e: Exception) {
                throw IllegalStateException(
                    "ledger dir $dir: cannot create (${e.message}) — " +
                        "durability is configured, refusing silent in-memory fallback (L11 fail-loud)",
                    e
                )
            }
            return LedgerJournal(dir.resolve("$group.jsonl"), ObjectMapper())
        }
    }

    /** The verified replay: bounded history material + the resumed head. */
    data class Replay(val events: List<ChatEvent>, val head: String?)

    @Synchronized
    fun append(e: ChatEvent, canonical: String, head: String) {
        val line = mapper.writeValueAsString(lineFields(e, canonical, head)) + "\n"
        try {
            channel.write(ByteBuffer.wrap(line.toByteArray(Charsets.UTF_8)))
            channel.force(true)
        } catch (t: Throwable) {
            throw IllegalStateException(
                "ledger journal $file: append failed (${t.message}) — " +
                    "the room refuses to lose a sealed event (L11 fail-loud)",
                t
            )
        }
    }

    private fun lineFields(e: ChatEvent, canonical: String, head: String): Map<String, Any?> = when (e) {
        is ChatEvent.Joined -> mapOf(
            "event" to "Joined", "group" to e.group, "handle" to e.handle, "at" to e.at.toString(),
        )
        is ChatEvent.Said -> mapOf(
            "event" to "Said", "group" to e.group, "handle" to e.handle, "at" to e.at.toString(),
            "text" to e.text, "formats" to e.formats, "links" to e.links,
        )
        is ChatEvent.Thought -> mapOf(
            "event" to "Thought", "group" to e.group, "handle" to e.handle, "at" to e.at.toString(),
            "trace" to e.trace,
        )
        is ChatEvent.Left -> mapOf(
            "event" to "Left", "group" to e.group, "handle" to e.handle, "at" to e.at.toString(),
        )
    } + mapOf("canonical" to canonical, "head" to head)

    /** Replay and verify. Throws [IllegalStateException] naming file and line on tamper. */
    fun replay(): Replay {
        if (!Files.exists(file)) return Replay(emptyList(), null)
        val raw = Files.readAllBytes(file)
        if (raw.isEmpty()) return Replay(emptyList(), null)
        val text = String(raw, Charsets.UTF_8)
        val offsets = lineOffsets(text)
        var prev: String? = null
        val events = ArrayList<ChatEvent>(offsets.size)
        offsets.forEachIndexed { idx, (start, end, terminated) ->
            val body = text.substring(start, end)
            val isFinal = idx == offsets.lastIndex
            val parsed = try {
                readValue(body)
            } catch (e: Exception) {
                if (isFinal && !terminated) {
                    // torn tail from a crash mid-write: truncate loudly, boot clean
                    System.err.println(
                        "chorux ledger: $file line ${idx + 1} is a torn write (no trailing newline) — " +
                            "truncating the crash artifact and replaying the verified prefix"
                    )
                    channel.truncate(start.toLong())
                    return Replay(events.toList(), prev)
                }
                throw IllegalStateException(
                    "ledger journal $file line ${idx + 1}: unparseable (${e.message}) — " +
                        "the chain refuses to resume over a broken middle (tamper-evidence, L10)",
                    e
                )
            }
            val canonical = str(parsed, "canonical")
            val head = str(parsed, "head")
            val expect = sha256((prev ?: "") + canonical)
            if (expect != head) {
                throw IllegalStateException(
                    "ledger journal $file line ${idx + 1}: chain mismatch — " +
                        "expected head $expect, journal says $head. Quiet edits re-head the chain (L10); " +
                        "the room refuses to form over a broken chain"
                )
            }
            events.add(eventOf(parsed))
            prev = head
        }
        return Replay(events.toList(), prev)
    }

    private fun readValue(body: String): Map<*, *> =
        mapper.readValue(body, Map::class.java)

    private fun str(m: Map<*, *>, k: String): String =
        m[k] as? String ?: throw IllegalStateException("field '$k' missing")

    @Suppress("UNCHECKED_CAST")
    private fun eventOf(m: Map<*, *>): ChatEvent {
        val group = str(m, "group"); val handle = str(m, "handle")
        val at = java.time.Instant.parse(str(m, "at"))
        return when (val kind = str(m, "event")) {
            "Joined" -> ChatEvent.Joined(group, handle, at)
            "Said" -> ChatEvent.Said(
                group, handle,
                str(m, "text"),
                (m["formats"] as? List<*>)?.map { it.toString() } ?: emptyList(),
                (m["links"] as? List<*>)?.map { it.toString() } ?: emptyList(),
                at,
            )
            "Thought" -> ChatEvent.Thought(group, handle, str(m, "trace"), at)
            "Left" -> ChatEvent.Left(group, handle, at)
            else -> throw IllegalStateException("unknown event kind '$kind' in $file")
        }
    }

    private fun lineOffsets(text: String): List<Triple<Int, Int, Boolean>> {
        val out = ArrayList<Triple<Int, Int, Boolean>>()
        var start = 0
        var i = text.indexOf('\n')
        while (i >= 0) {
            out.add(Triple(start, i, true))
            start = i + 1
            i = text.indexOf('\n', start)
        }
        if (start < text.length) out.add(Triple(start, text.length, false))
        return out
    }

    private fun sha256(s: String): String =
        MessageDigest.getInstance("SHA-256")
            .digest(s.toByteArray(Charsets.UTF_8))
            .joinToString("") { "%02x".format(it) }
}
