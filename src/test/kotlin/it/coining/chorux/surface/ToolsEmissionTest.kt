package it.coining.chorux.surface

import it.coining.chorux.message.ChatEvent
import it.coining.chorux.message.EventTransport
import it.coining.chorux.room.RoomRegistry
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

/**
 * L9 at the seam: only a member's intentional act (join/say) reaches the
 * transport — the exact recorded event, never a copy — and `pulse` emits
 * nothing, because responses are pull-only.
 */
class ToolsEmissionTest {

    private class RecordingTransport : EventTransport {
        val events = mutableListOf<ChatEvent>()
        override fun emit(event: ChatEvent) { events.add(event) }
    }

    private val registry = RoomRegistry(historyLimit = 10)
    private val transport = RecordingTransport()
    private val tools = ChatRoomTools(registry, listOf(transport))

    @Test
    fun `join emits the member's arrival`() {
        tools.join("g", "omo")
        assertEquals(1, transport.events.size)
        assertIs<ChatEvent.Joined>(transport.events.single())
    }

    @Test
    fun `say emits the exact event the room recorded - and the thought too`() {
        tools.join("g", "omo")
        transport.events.clear()
        val result = tools.say("g", "omo", "the wire carries what the room recorded", listOf("claim#parity"), "thinking")
        assertIs<ChatEvent.Said>(transport.events[0])
        assertEquals(result.said.text, (transport.events[0] as ChatEvent.Said).text)
        assertEquals(result.said.at, (transport.events[0] as ChatEvent.Said).at)
        assertEquals(result.said.links, (transport.events[0] as ChatEvent.Said).links)
        assertIs<ChatEvent.Thought>(transport.events[1])
    }

    @Test
    fun `pulse never emits - responses are pull-only, L9`() {
        tools.join("g", "omo")
        tools.say("g", "omo", "words")
        transport.events.clear()
        tools.pulse("g")
        assertTrue(transport.events.isEmpty())
    }
}
