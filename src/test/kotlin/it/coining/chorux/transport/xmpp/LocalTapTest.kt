package it.coining.chorux.transport.xmpp

import it.coining.chorux.message.ChatEvent
import it.coining.chorux.message.EventTransport
import it.coining.chorux.room.RoomRegistry
import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs

/** docs/07: local taps hear the whole room — ingested rail acts included — while the rail never re-emits what it ingested. */
class LocalTapTest {

    private class RecordingTap : EventTransport {
        val events = mutableListOf<ChatEvent>()
        override fun emit(event: ChatEvent) { events.add(event) }
    }

    private fun adapter(taps: List<EventTransport>) = XmppTransportProperties(
        enabled = true, host = "h", serviceName = "s", username = "u",
        password = "p", mucDomain = "m", nick = "n",
    ).let { XmppEventTransport(it, RoomRegistry(10), taps) }

    @Test
    fun `an ingested rail act reaches the local sink`() {
        val tap = RecordingTap()
        adapter(listOf(tap)).tapLocal(ChatEvent.Said("g", "peer", "the crossing turn", at = Instant.now()))
        val heard = assertIs<ChatEvent.Said>(tap.events.single())
        assertEquals("the crossing turn", heard.text)
        assertEquals("peer", heard.handle)
    }

    @Test
    fun `the tap list never includes the rail adapter itself`() {
        val tap = RecordingTap()
        val a = adapter(listOf(tap))
        a.tapLocal(ChatEvent.Said("g", "peer", "x", at = Instant.now()))
        assertEquals(1, tap.events.size)
    }
}
