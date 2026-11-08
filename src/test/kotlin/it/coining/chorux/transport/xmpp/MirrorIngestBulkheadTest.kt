package it.coining.chorux.transport.xmpp

import it.coining.chorux.message.ChatEvent
import it.coining.chorux.room.RoomRegistry
import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertTrue

/**
 * The bulkhead law (measured live): a claim-refusal
 * during mirror ingest NEVER terminates the process — the event is
 * dead-lettered and the room keeps serving. The incident class: a seat
 * directly claimed on one daemon while the rail mirror relays its words from
 * another; the lockout law refuses the mirror's claim (correct law) and the
 * refusal must be loud and survivable, never a process death. The survival
 * receipt: live rooms logged the ISE refusals and held 0 restarts — the
 * catch holds; this test pins it so it can never regress silently.
 */
class MirrorIngestBulkheadTest {

    private fun transport(registry: RoomRegistry) = XmppTransportProperties(
        enabled = true, host = "h", port = 5222, serviceName = "s", username = "u",
        password = "p", mucDomain = "m", nick = "n",
    ).let { XmppEventTransport(it, registry, emptyList()) }

    @Test
    fun `claim refusal dead-letters and never escapes — ISE class`() {
        val registry = RoomRegistry(50)
        registry.formOrJoin("ee-swarm", "vip", "ses-direct") // a direct session holds vip
        val t = transport(registry)

        // the mirror tries to ingest vip's words from the rail — the lockout law
        // refuses, the bulkhead catches, nothing escapes
        t.ingest(ChatEvent.Said("ee-swarm", "vip", "the star speaks from the rail", at = Instant.now()), "ee-swarm", "vip")

        val recent = registry.room("ee-swarm")!!.pulse().recent
        assertTrue(
            recent.none { (it as? ChatEvent.Said)?.text?.contains("the star speaks") == true },
            "a refused mirror say must not land in the room — the origin daemon's ledger is the truth"
        )
    }

    @Test
    fun `after a refusal the very next mirrored say still lands — keep serving`() {
        val registry = RoomRegistry(50)
        registry.formOrJoin("ee-swarm", "vip", "ses-direct")
        val t = transport(registry)
        t.ingest(ChatEvent.Said("ee-swarm", "vip", "refused words", at = Instant.now()), "ee-swarm", "vip")

        t.ingest(ChatEvent.Said("ee-swarm", "free-lane", "still dancing", at = Instant.now()), "ee-swarm", "free-lane")

        val recent = registry.room("ee-swarm")!!.pulse().recent
        assertTrue(
            recent.any { (it as? ChatEvent.Said)?.text == "still dancing" },
            "the bulkhead must not poison the next ingest — the room keeps serving"
        )
    }

    @Test
    fun `unseated origin handle dead-letters instead of escaping — IAE class`() {
        val registry = RoomRegistry(50)
        val t = transport(registry)

        // envelope handle never seated under the mirror session: the from-nick
        // joins, the origin say is refused (say-as-held) — caught, never thrown
        t.ingest(ChatEvent.Said("ee-swarm", "ghost-lane", "who holds me?", at = Instant.now()), "ee-swarm", "rail-mirror-nick")

        assertTrue(
            registry.room("ee-swarm")!!.pulse().recent.none { (it as? ChatEvent.Said)?.text == "who holds me?" },
            "an unseated origin handle must dead-letter, never escape the callback"
        )
    }
}
