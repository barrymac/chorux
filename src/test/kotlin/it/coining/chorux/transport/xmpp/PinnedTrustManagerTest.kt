package it.coining.chorux.transport.xmpp

import it.coining.chorux.room.RoomRegistry
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** docs/07: the trust pin loads real PEM material into a scoped manager — the failure mode that cost E2E runs 3 and 4, pinned at unit level. */
class PinnedTrustManagerTest {

    private fun props(trust: String) = XmppTransportProperties(
        enabled = true, host = "h", serviceName = "s", username = "u",
        password = "p", mucDomain = "m", nick = "n", trustCaPem = trust,
    )

    @Test
    fun `a pinned PEM loads into a trust manager with accepted issuers`() {
        val tm = XmppEventTransport(props("src/test/resources/test-ca.crt"), RoomRegistry(10)).pinnedTrustManager()
        assertTrue(tm != null)
        assertEquals(1, tm!!.acceptedIssuers.size)
    }

    @Test
    fun `absent pin means JVM default trust - no scoped manager`() {
        assertNull(XmppEventTransport(props(""), RoomRegistry(10)).pinnedTrustManager())
    }

    @Test
    fun `PEM CONTENT form loads - the kubernetes secret shape, env carries the cert itself`() {
        val pem = File("src/test/resources/test-ca.crt").readText()
        val tm = XmppEventTransport(props(pem), RoomRegistry(10)).pinnedTrustManager()
        assertTrue(tm != null)
        assertEquals(1, tm!!.acceptedIssuers.size)
    }

    @Test
    fun `a pin that is neither PEM content nor an existing file fails loud at startup`() {
        val ex = assertFailsWith<IllegalArgumentException> {
            XmppEventTransport(props("/no/such/ca.pem"), RoomRegistry(10)).pinnedTrustManager()
        }
        assertTrue(ex.message!!.contains("neither PEM content nor an existing file"))
    }

    @Test
    fun `PEM-shaped noise parsing zero certificates fails loud - never an empty trust anchor`() {
        val ex = assertFailsWith<IllegalArgumentException> {
            XmppEventTransport(props("-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n"), RoomRegistry(10)).pinnedTrustManager()
        }
        assertTrue(ex.message!!.contains("no parseable certificate"))
    }
}
