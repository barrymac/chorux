package it.coining.chorux.transport.xmpp

import it.coining.chorux.message.ChatEvent
import it.coining.chorux.room.RoomRegistry
import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertTrue

/**
 * The tolerance law (decreed 2026-10-29 against the measured fork-starved
 * reactor wedge): the wall never waits on the glass. Emission is a queue
 * offer, boot succeeds with the rail down, and the supervisor owns every
 * redial in the background. The numbers here pin the p99 contract: fifty
 * emits must cost microseconds, no matter the rail's health.
 */
class MirrorSupervisionTest {

    private fun adapter(host: String = "h", port: Int = 5222) = XmppTransportProperties(
        enabled = true, host = host, port = port, serviceName = "s", username = "u",
        password = "p", mucDomain = "m", nick = "n",
    ).let { XmppEventTransport(it, RoomRegistry(10), emptyList()) }

    private val said = ChatEvent.Said("chorux", "omo", "the wall never waits", at = Instant.now())

    @Test
    fun `emit never blocks the request thread — never-dialed rail, fifty emits under half a second`() {
        val t = adapter()
        val start = System.nanoTime()
        repeat(50) { t.emit(said) }
        val elapsedMs = (System.nanoTime() - start) / 1_000_000
        assertTrue(elapsedMs < 500, "50 emits took ${elapsedMs}ms — the wall waited on the glass")
    }

    @Test
    fun `boot tolerance — an unreachable rail never kills the room, emits still queue`() {
        val t = adapter(host = "127.0.0.1", port = 1)
        t.start()
        assertTrue(t.isRunning, "the room must run with the rail down")
        val start = System.nanoTime()
        t.emit(said)
        val elapsedMs = (System.nanoTime() - start) / 1_000_000
        assertTrue(elapsedMs < 500, "emit behind a dead rail took ${elapsedMs}ms")
        t.stop()
    }
}
