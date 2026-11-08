package it.coining.chorux.surface

import io.modelcontextprotocol.server.McpAsyncServerExchange
import io.modelcontextprotocol.server.McpSyncServerExchange
import it.coining.chorux.message.ChatEvent
import it.coining.chorux.message.EventTransport
import it.coining.chorux.room.RoomRegistry
import org.springframework.ai.chat.model.ToolContext
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

/**
 * docs/10: the dialogue tier's off-wall verb — a sayTo mints no room event
 * (no ledger entry, no transport tap hears it), delivers only through
 * occupant-capable transports, and the seam's default refusal IS the lawful
 * UNDELIVERABLE, aggregated loud — never a silent degradation.
 */
class SayToToolTest {

    private class RecordingTransport : EventTransport {
        val events = mutableListOf<ChatEvent>()
        val direct = mutableListOf<Pair<ChatEvent.Said, String>>()
        override fun emit(event: ChatEvent) { events.add(event) }
        override fun sayTo(event: ChatEvent.Said, nick: String) { direct.add(event to nick) }
    }

    private class RefusingTransport : EventTransport {
        val events = mutableListOf<ChatEvent>()
        override fun emit(event: ChatEvent) { events.add(event) }
    }

    /** L19 fixture: the live MCP exchange Spring AI puts into every tool call's ToolContext. */
    private fun ctx(session: String): ToolContext =
        ToolContext(mapOf("exchange" to McpSyncServerExchange(McpAsyncServerExchange(session, null, null, null, null))))

    private fun tools(vararg transports: EventTransport) =
        ChatRoomTools(RoomRegistry(historyLimit = 10), transports.toList())

    @Test
    fun `sayTo delivers off-wall and mints no room event`() {
        val recording = RecordingTransport()
        val tools = tools(recording)
        tools.join("g", "omo", ctx("ses-omo"))
        recording.events.clear()

        val result = tools.sayTo("g", "omo", "pilot", "hop 2, off-wall", ctx("ses-omo"))

        assertEquals("pilot", result.to)
        assertEquals("hop 2, off-wall", result.text)
        assertEquals(listOf("RecordingTransport"), result.deliveredVia)
        assertEquals(1, recording.direct.size)
        assertEquals("pilot", recording.direct.single().second)
        // off-wall (docs/10): no emission, no journal entry
        assertEquals(0, recording.events.size)
        assertTrue(
            tools.pulse("g").recent.none { it is ChatEvent.Said && it.text == "hop 2, off-wall" },
            "the direct reply must never appear on the room wall",
        )
    }

    @Test
    fun `the default refusal is the lawful UNDELIVERABLE, aggregated loud`() {
        val tools = tools(RefusingTransport())
        tools.join("g", "omo", ctx("ses-omo"))
        val e = assertFailsWith<IllegalStateException> { tools.sayTo("g", "omo", "pilot", "x", ctx("ses-omo")) }
        assertTrue(e.message!!.startsWith("UNDELIVERABLE"), "got: ${e.message}")
        assertTrue("RefusingTransport" in e.message!!, "the refusing transport must be named, got: ${e.message}")
    }

    @Test
    fun `a group the seat never joined is refused loud`() {
        val e = assertFailsWith<IllegalStateException> { tools(RecordingTransport()).sayTo("nope", "omo", "pilot", "x", ctx("ses-omo")) }
        assertTrue("no such group" in e.message!!)
    }

    @Test
    fun `sayTo as a seat this connection does not hold is refused loud`() {
        val recording = RecordingTransport()
        val tools = tools(recording)
        tools.join("g", "omo", ctx("ses-omo"))
        val e = assertFailsWith<IllegalStateException> { tools.sayTo("g", "omo", "pilot", "x", ctx("ses-evil")) }
        assertTrue("does not hold" in e.message!!, "must name the say-as-held law, got: ${e.message}")
        assertTrue(recording.direct.isEmpty(), "an impersonated off-wall reply delivers nothing")
    }
}
