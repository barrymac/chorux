package it.coining.chorux.transport.xmpp

import it.coining.chorux.message.ChatEvent
import it.coining.chorux.room.RoomRegistry
import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

/**
 * docs/10 + docs/07: identity stays bytes for nicks too — the occupant
 * address is built through the rail's own parser and must roundtrip
 * byte-exact (room@conference-domain/nick); a blank nick or a stopped
 * transport is UNDELIVERABLE, refused loud — never a silent degradation.
 */
class SayToOccupantTest {

    private fun adapter() = XmppTransportProperties(
        enabled = true, host = "h", serviceName = "s", username = "u",
        password = "p", mucDomain = "m", nick = "n",
    ).let { XmppEventTransport(it, RoomRegistry(10), emptyList()) }

    private val said = ChatEvent.Said("chorux", "omo", "hop 2", at = Instant.now())

    @Test
    fun `the occupant address is room@domain slash nick, byte-exact`() {
        val jid = adapter().occupantJid("chorux", "keel")
        assertEquals("chorux@m", jid.toString().substringBefore("/"))
        assertEquals("keel", jid.resourcepart.toString())
    }

    @Test
    fun `a blank nick is UNDELIVERABLE`() {
        val e = assertFailsWith<IllegalArgumentException> { adapter().sayTo(said, " ") }
        assertTrue(e.message!!.startsWith("UNDELIVERABLE"), "got: ${e.message}")
    }

    @Test
    fun `a stopped transport is UNDELIVERABLE, never silent`() {
        val e = assertFailsWith<IllegalStateException> { adapter().sayTo(said, "keel") }
        assertTrue(e.message!!.startsWith("UNDELIVERABLE"), "got: ${e.message}")
    }
}
