package it.coining.chorux.room

import kotlin.reflect.full.functions
import kotlin.reflect.KVisibility
import it.coining.chorux.message.ChatEvent
import org.junit.jupiter.api.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

class ChatRoomTest {

    @Test
    fun `group formation is reflexive — the first join creates the group`() {
        val r = RoomRegistry(500)
        val (room, formed) = r.formOrJoin("open-space", "vee")
        assertTrue(formed)
        assertNotNull(room)
        val (_, formedAgain) = r.formOrJoin("open-space", "vee")
        val (afterSecond, thirdJoiner) = r.formOrJoin("open-space", "ukhin", )
        assertFalse(formedAgain)
        assertFalse(thirdJoiner)
        assertEquals(2, afterSecond.pulse().occupants.size)
    }

    @Test
    fun `zero-mapping guard — slash groups are refused loudly at formation, never escaped`() {
        val r = RoomRegistry(500)
        val ex = kotlin.test.assertFailsWith<IllegalArgumentException> {
            r.formOrJoin("chorux/melanoi", "omo")
        }
        assertTrue(ex.message!!.contains("chorux/melanoi"), "error must name the group, got: ${ex.message}")
        assertTrue(ex.message!!.contains("zero-mapping"), "error must cite the law, got: ${ex.message}")
        // the room never formed: no silent mirror-failure downstream
        kotlin.test.assertNull(r.room("chorux/melanoi"))
    }

    @Test
    fun `zero-mapping guard — rail-folded names are refused, identity stays bytes`() {
        val r = RoomRegistry(500)
        val ex = kotlin.test.assertFailsWith<IllegalArgumentException> {
            r.formOrJoin("Genesis", "omo")
        }
        assertTrue(ex.message!!.contains("genesis"), "error must show the folded form, got: ${ex.message}")
        kotlin.test.assertNull(r.room("Genesis"))
        kotlin.test.assertNull(r.room("genesis"))
    }

    @Test
    fun `zero-mapping guard — other JID-localpart-illegal characters are refused, legal names still form`() {
        val r = RoomRegistry(500)
        listOf("dog house", "x@muc", "a:b", "q<z").forEach { bad ->
            kotlin.test.assertFailsWith<IllegalArgumentException>(
                message = "expected refusal for '$bad'"
            ) { r.formOrJoin(bad, "omo") }
        }
        val (_, formed) = r.formOrJoin("melanoi", "omo")
        assertTrue(formed, "a legal localpart must still form reflexively")
    }

    @Test
    fun `payloads pass through unmodified — the room never tells people what to say`() {
        val room = ChatRoom("plain", 100)
        room.join("anon")
        val weird = "anything: 中文 🎭 ```bash rm -rf /``` <script> /status irrelevant — opaque by law"
        val said = room.say("anon", weird)
        assertEquals(weird, said.text)
        assertEquals(weird, (room.pulse().recent.filterIsInstance<ChatEvent.Said>().last()).text)
    }

    @Test
    fun `event ledger is tamper-evident — every new event re-heads the chain`() {
        val room = ChatRoom("chain", 100)
        room.join("a"); room.join("b")
        val h1 = room.pulse().ledgerHead
        room.say("a", "first")
        val h2 = room.pulse().ledgerHead
        room.think("b", "second, with thinking")
        val h3 = room.pulse().ledgerHead
        assertTrue(listOf(h1, h2, h3).mapNotNull { it }.toSet().size == 3)
        assertTrue((h3 ?: "").matches(Regex("[0-9a-f]{64}")))
    }

    @Test
    fun `history is bounded — unlimited growth is refused quietly-old, never quietly-dead`() {
        val room = ChatRoom("bounded", 5)
        room.join("a") // one event in history already
        repeat(10) { room.say("a", "msg-$it") }
        assertTrue(room.pulse().recent.size <= 5)
        assertNotNull(room.pulse().ledgerHead) // chain survives trimming — trimming is not editing
    }

    @Test
    fun `microformats ride declared, never interpreted — including emergent invented ones`() {
        val room = ChatRoom("fmt", 100)
        room.join("anon")
        val said = room.say("anon", "see [[card:42cf8cd0-86dc]] for the design",
            formats = listOf("link#atom", "invented#my-thing", "claim#work-graph"))
        assertEquals(listOf("link#atom", "invented#my-thing", "claim#work-graph"), said.formats)
        // the labels arrive untouched — that IS the support
        val back = room.pulse().recent.filterIsInstance<ChatEvent.Said>().last()
        assertEquals(said.formats, back.formats)
    }

    @Test
    fun `neurOS-speak is lifted mechanically and surfaces in the pulse — free`() {
        val room = ChatRoom("links", 100)
        room.join("substrate-speaker")
        val said = room.say("substrate-speaker", "blocked by [[blocks:9d2f]] and tracked at [[tracks:042]] — also [[weird[[scope? fine")
        assertEquals(listOf("blocks:9d2f", "tracks:042"), said.links)
        assertEquals(listOf("blocks:9d2f", "tracks:042"), room.pulse().recentLinks)
    }

    @Test
    fun `unsolicited broadcasts are structurally impossible — ChatRoom has no emission surface at all`() {
        // Law: only asked-for output exists. Everything here is a function return.
        // This test exists to anchor the law: ChatRoom offers join/say/think/pulse —
        // nothing else appears in its public function API, so nothing can push.
        val fns = ChatRoom::class.functions.filter { it.visibility == KVisibility.PUBLIC }.map { it.name }.toSet()
        assertEquals(setOf("join", "say", "think", "pulse"), fns - setOf("equals", "hashCode", "toString"))
        assertFalse(fns.any { it in setOf("notify", "broadcast", "push", "subscribe", "emit", "send") })
    }
}
