package it.coining.chorux.transport.xmpp

import it.coining.chorux.message.ChatEvent
import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** docs/07 oracle 1: parity — an event through the wire and back is field-identical. */
class WireEnvelopeTest {

    private val at = Instant.parse("2026-10-29T04:44:44Z")

    @Test
    fun `a Said crosses the wire field-identical`() {
        val said = ChatEvent.Said("chorux/genesis", "omo", "payloads stay opaque", listOf("poke#ci-lane"), listOf("asks:ci-lane"), at)
        val decoded = WireEnvelope.decode(WireEnvelope.encode(said)!!, "chorux/genesis", "someone")
        val back = assertIs<ChatEvent.Said>(decoded)
        assertEquals("chorux/genesis", back.group)
        assertEquals("omo", back.handle)
        assertEquals("payloads stay opaque", back.text)
        assertEquals(listOf("poke#ci-lane"), back.formats)
        assertEquals(listOf("asks:ci-lane"), back.links)
        assertEquals(at, back.at)
    }

    @Test
    fun `a Thought crosses the wire field-identical`() {
        val thought = ChatEvent.Thought("chorux/genesis", "omo", "the trace is a first-class act", at)
        val back = assertIs<ChatEvent.Thought>(WireEnvelope.decode(WireEnvelope.encode(thought)!!, "chorux/genesis", "someone"))
        assertEquals("the trace is a first-class act", back.trace)
        assertEquals("omo", back.handle)
        assertEquals(at, back.at)
    }

    @Test
    fun `a Joined crosses the wire field-identical`() {
        val joined = ChatEvent.Joined("chorux/genesis", "omo", at)
        val back = assertIs<ChatEvent.Joined>(WireEnvelope.decode(WireEnvelope.encode(joined)!!, "chorux/genesis", "someone"))
        assertEquals("omo", back.handle)
        assertEquals(at, back.at)
    }

    @Test
    fun `Left never travels`() {
        assertNull(WireEnvelope.encode(ChatEvent.Left("chorux/genesis", "omo", at)))
    }

    @Test
    fun `a plain chat line from a human client becomes their Said - the L18 human half`() {
        val back = assertIs<ChatEvent.Said>(WireEnvelope.decode("any client, any server, just words", "chorux/genesis", "bioceo-p"))
        assertEquals("bioceo-p", back.handle)
        assertEquals("any client, any server, just words", back.text)
    }

    @Test
    fun `an unknown envelope degrades to an opaque plain Say - never an interpretation`() {
        val foreign = """{"event":"SaidFromTheFuture","group":"g","handle":"x","at":"2026-10-29T04:44:44Z","text":"hi"}"""
        val back = assertIs<ChatEvent.Said>(WireEnvelope.decode(foreign, "g", "someone"))
        assertEquals(foreign, back.text)
        assertTrue(back.text.contains("SaidFromTheFuture"))
    }
}
