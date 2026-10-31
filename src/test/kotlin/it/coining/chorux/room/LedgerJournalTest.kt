package it.coining.chorux.room

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.io.TempDir
import java.nio.file.Files
import java.nio.file.Path
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

class LedgerJournalTest {

    @TempDir
    lateinit var tmp: Path

    private fun room(dir: Path? = null) =
        if (dir == null) ChatRoom("chorux", 500)
        else ChatRoom("chorux", 500, LedgerJournal.open(dir, "chorux"))

    @Test
    fun `history and chain head survive a restart — occupants do not`() {
        val dir = tmp.resolve("ledger")
        val r1 = room(dir)
        r1.join("keel")
        r1.say("keel", "hello [[edge:x]]", formats = listOf("poke#omo"))
        r1.think("keel", "tracing")
        val headBefore = r1.pulse().ledgerHead
        assertTrue(headBefore != null)

        val r2 = room(dir)
        val p = r2.pulse()
        assertEquals(headBefore, p.ledgerHead, "chain head must resume from the journal")
        assertTrue(p.recent.any { it is it.coining.chorux.message.ChatEvent.Said && it.handle == "keel" },
            "sealed says must survive restarts")
        assertTrue(p.recentThoughts.any { it.handle == "keel" }, "thoughts must survive restarts")
        assertEquals(listOf("edge:x"), p.recentLinks, "lifted links must survive restarts")
        assertTrue(p.occupants.isEmpty(), "occupants are live facts — never replayed (L19)")
    }

    @Test
    fun `a tampered head refuses the boot, naming file and line`() {
        val dir = tmp.resolve("ledger")
        val r1 = room(dir)
        r1.join("keel")
        r1.say("keel", "one")
        r1.say("keel", "two")
        val f = dir.resolve("chorux.jsonl")
        val lines = Files.readAllLines(f)
        // quietly edit the middle line's head — exactly what L10 exists to catch
        val edited = lines[1].replace(Regex("\"head\":\"[0-9a-f]+\""), "\"head\":\"deadbeef\"")
        Files.write(f, listOf(lines[0], edited, lines[2]))

        val ex = assertFailsWith<IllegalStateException> { room(dir) }
        assertTrue((ex.message ?: "").contains("chain mismatch"), "got: ${ex.message}")
        assertTrue((ex.message ?: "").contains("line 2"), "the refusal must name the line, got: ${ex.message}")
    }

    @Test
    fun `an unparseable middle line refuses the boot, a torn tail heals`() {
        val dir = tmp.resolve("ledger")
        val r1 = room(dir)
        r1.join("keel")
        r1.say("keel", "one")
        r1.say("keel", "two")
        val f = dir.resolve("chorux.jsonl")

        // torn tail: strip the final newline of the last line (crash mid-write)
        val raw = Files.readString(f)
        Files.writeString(f, raw.trimEnd('\n') + "\n{\"event\":\"Said\"") // valid prefix + torn fragment
        val r2 = room(dir) // must boot: prefix verified, artifact truncated
        assertTrue(r2.pulse().recent.isNotEmpty(), "verified prefix replays")

        // broken middle: refuse
        val lines2 = Files.readAllLines(f).toMutableList()
        lines2[0] = "{not json"
        Files.write(f, lines2)
        val ex = assertFailsWith<IllegalStateException> { room(dir) }
        assertTrue((ex.message ?: "").contains("unparseable"), "got: ${ex.message}")
    }

    @Test
    fun `durability configured but unwritable fails loud, never falls back silently`() {
        val notADir = tmp.resolve("blocker")
        Files.writeString(notADir, "occupied")
        val ex = assertFailsWith<IllegalStateException> { LedgerJournal.open(notADir, "chorux") }
        assertTrue((ex.message ?: "").contains("refusing silent in-memory fallback"), "got: ${ex.message}")
    }

    @Test
    fun `no dir means honest in-memory day one — everything works, nothing persisted`() {
        val r = room()
        r.join("vee")
        r.say("vee", "plain")
        assertTrue((r.pulse().ledgerHead ?: "").isNotEmpty(), "the chain still runs in memory")
        assertTrue(r.pulse().recent.any { it.handle == "vee" })
    }
}
