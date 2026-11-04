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
        val (room, formed) = r.formOrJoin("open-space", "vee", "ses-vee")
        assertTrue(formed)
        assertNotNull(room)
        val (_, formedAgain) = r.formOrJoin("open-space", "vee", "ses-vee")
        val (afterSecond, thirdJoiner) = r.formOrJoin("open-space", "ukhin", "ses-ukhin")
        assertFalse(formedAgain)
        assertFalse(thirdJoiner)
        assertEquals(2, afterSecond.pulse().occupants.size)
    }

    @Test
    fun `zero-mapping guard — slash groups are refused loudly at formation, never escaped`() {
        val r = RoomRegistry(500)
        val ex = kotlin.test.assertFailsWith<IllegalArgumentException> {
            r.formOrJoin("chorux/melanoi", "omo", "ses-omo")
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
            r.formOrJoin("Genesis", "omo", "ses-omo")
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
            ) { r.formOrJoin(bad, "omo", "ses-omo") }
        }
        val (_, formed) = r.formOrJoin("melanoi", "omo", "ses-omo")
        assertTrue(formed, "a legal localpart must still form reflexively")
    }

    @Test
    fun `payloads pass through unmodified — the room never tells people what to say`() {
        val room = ChatRoom("plain", 100)
        room.join("anon", "ses-anon")
        val weird = "anything: 中文 🎭 ```bash rm -rf /``` <script> /status irrelevant — opaque by law"
        val said = room.say("anon", "ses-anon", weird)
        assertEquals(weird, said.text)
        assertEquals(weird, (room.pulse().recent.filterIsInstance<ChatEvent.Said>().last()).text)
    }

    @Test
    fun `event ledger is tamper-evident — every new event re-heads the chain`() {
        val room = ChatRoom("chain", 100)
        room.join("a", "ses-a"); room.join("b", "ses-b")
        val h1 = room.pulse().ledgerHead
        room.say("a", "ses-a", "first")
        val h2 = room.pulse().ledgerHead
        room.think("b", "ses-b", "second, with thinking")
        val h3 = room.pulse().ledgerHead
        assertTrue(listOf(h1, h2, h3).mapNotNull { it }.toSet().size == 3)
        assertTrue((h3 ?: "").matches(Regex("[0-9a-f]{64}")))
    }

    @Test
    fun `history is bounded — unlimited growth is refused quietly-old, never quietly-dead`() {
        val room = ChatRoom("bounded", 5)
        room.join("a", "ses-a") // one event in history already
        repeat(10) { room.say("a", "ses-a", "msg-$it") }
        assertTrue(room.pulse().recent.size <= 5)
        assertNotNull(room.pulse().ledgerHead) // chain survives trimming — trimming is not editing
    }

    @Test
    fun `microformats ride declared, never interpreted — including emergent invented ones`() {
        val room = ChatRoom("fmt", 100)
        room.join("anon", "ses-anon")
        val said = room.say("anon", "ses-anon", "see [[card:42cf8cd0-86dc]] for the design",
            formats = listOf("link#atom", "invented#my-thing", "claim#work-graph"))
        assertEquals(listOf("link#atom", "invented#my-thing", "claim#work-graph"), said.formats)
        // the labels arrive untouched — that IS the support
        val back = room.pulse().recent.filterIsInstance<ChatEvent.Said>().last()
        assertEquals(said.formats, back.formats)
    }

    @Test
    fun `neurOS-speak is lifted mechanically and surfaces in the pulse — free`() {
        val room = ChatRoom("links", 100)
        room.join("substrate-speaker", "ses-ss")
        val said = room.say("substrate-speaker", "ses-ss", "blocked by [[blocks:9d2f]] and tracked at [[tracks:042]] — also [[weird[[scope? fine")
        assertEquals(listOf("blocks:9d2f", "tracks:042"), said.links)
        assertEquals(listOf("blocks:9d2f", "tracks:042"), room.pulse().recentLinks)
    }

    @Test
    fun `unsolicited broadcasts are structurally impossible — ChatRoom has no emission surface at all`() {
        // Law: only asked-for output exists. Everything here is a function return.
        // This test exists to anchor the law: ChatRoom offers join/say/think/pulse
        // plus the seat's release and the say-as-held check — nothing else appears
        // in its public function API, so nothing can push.
        val fns = ChatRoom::class.functions.filter { it.visibility == KVisibility.PUBLIC }.map { it.name }.toSet()
        assertEquals(setOf("join", "say", "think", "pulse", "release", "requireHeld"), fns - setOf("equals", "hashCode", "toString"))
        assertFalse(fns.any { it in setOf("notify", "broadcast", "push", "subscribe", "emit", "send") })
    }

    @Test
    fun `one live claim per handle — a second claim while the first is live is refused loud`() {
        val r = RoomRegistry(500)
        r.formOrJoin("two-connections", "lane/a", "ses-a")
        val ex = kotlin.test.assertFailsWith<IllegalStateException> {
            r.formOrJoin("two-connections", "lane/a", "ses-b")
        }
        assertTrue("one live claim" in ex.message!!, "must cite the exclusivity law, got: ${ex.message}")
        assertTrue(r.room("two-connections")!!.pulse().occupants.any { it.handle == "lane/a" },
            "the claimant keeps standing — the refused claim changed nothing")
        val (_, formedB) = r.formOrJoin("two-connections", "lane/b", "ses-b")
        assertFalse(formedB, "a different handle is free to claim a seat in the same room")
    }

    @Test
    fun `re-claim by the same session is idempotent — reconnects are not double-joins`() {
        val room = ChatRoom("reclaim", 100)
        room.join("omo", "ses-1")
        room.join("omo", "ses-1")
        assertEquals(1, room.pulse().occupants.size)
        assertEquals(1, room.pulse().recent.count { it is ChatEvent.Joined })
    }

    @Test
    fun `say-as-held — a connection that does not hold the seat cannot speak as it`() {
        val room = ChatRoom("held", 100)
        room.join("lane/a", "ses-a")
        room.join("lane/b", "ses-b")
        val ex = kotlin.test.assertFailsWith<IllegalStateException> { room.say("lane/a", "ses-b", "as another") }
        assertTrue("does not hold" in ex.message!!, "must name the law, got: ${ex.message}")
        val exThink = kotlin.test.assertFailsWith<IllegalStateException> { room.think("lane/a", "ses-b", "trace as another") }
        assertTrue("does not hold" in exThink.message!!, "must name the law, got: ${exThink.message}")
        // the holder speaks fine; the impersonated say never happened
        room.say("lane/a", "ses-a", "as myself")
        assertFalse(room.pulse().recent.any { it is ChatEvent.Said && it.text == "as another" })
    }

    @Test
    fun `connection end releases the seats it claimed — Left is recorded, the handle is free`() {
        val r = RoomRegistry(500)
        r.formOrJoin("release", "lane/a", "ses-a")
        r.formOrJoin("release", "lane/b", "ses-a")
        val headBefore = r.room("release")!!.pulse().ledgerHead
        val released = r.release("ses-a")
        assertEquals(setOf("release"), released.keys)
        assertEquals(setOf("lane/a", "lane/b"), released["release"]!!.toSet())
        val pulse = r.room("release")!!.pulse()
        assertTrue(pulse.occupants.isEmpty(), "disconnect vacates the seats it claimed")
        val lefts = pulse.recent.filterIsInstance<ChatEvent.Left>()
        assertEquals(setOf("lane/a", "lane/b"), lefts.map { it.handle }.toSet())
        assertTrue(pulse.ledgerHead != headBefore, "Left events re-head the chain — tamper-evident attribution")
        // the freed handle is claimable by a new connection
        r.formOrJoin("release", "lane/a", "ses-b")
        assertTrue(r.room("release")!!.pulse().occupants.any { it.handle == "lane/a" })
        // idempotent: a connection that held nothing releases nothing
        assertTrue(r.release("ses-ghost").isEmpty())
    }

    @Test
    fun `presence end releases one mirrored seat only — the mirror holds many`() {
        val room = ChatRoom("mirror", 100)
        room.join("rail-one", "xmpp-rail-mirror")
        room.join("rail-two", "xmpp-rail-mirror")
        assertTrue(room.release("rail-one", "xmpp-rail-mirror"))
        assertFalse(room.release("rail-one", "xmpp-rail-mirror"), "double release is a no-op, not an error")
        // a non-holder cannot release someone else's seat
        room.join("local", "ses-local")
        assertFalse(room.release("local", "xmpp-rail-mirror"))
        val pulse = room.pulse()
        assertEquals(listOf("rail-two", "local"), pulse.occupants.map { it.handle })
        assertTrue(pulse.recent.none { it is ChatEvent.Left && it.handle == "local" })
        assertEquals(1, pulse.recent.count { it is ChatEvent.Left && it.handle == "rail-one" })
    }
}
