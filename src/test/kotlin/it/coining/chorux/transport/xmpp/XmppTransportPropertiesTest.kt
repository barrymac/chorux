package it.coining.chorux.transport.xmpp

import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertFailsWith
import kotlin.test.fail

/** docs/01 L11 + card 9dd35db2: fail-loud, every missing value named at once, never the first only. */
class XmppTransportPropertiesTest {

    @Test
    fun `enabled with everything missing refuses startup naming all seven keys`() {
        val props = XmppTransportProperties(enabled = true)
        val e = assertFailsWith<IllegalStateException> { props.requireComplete() }
        listOf("host", "service-name", "username", "password", "muc-domain", "nick", "resource").forEach {
            assertContains(e.message ?: fail("message absent"), "chorux.transport.xmpp.$it")
        }
    }

    @Test
    fun `enabled with one missing names exactly that one`() {
        val props = XmppTransportProperties(enabled = true, host = "h", serviceName = "s", username = "u", mucDomain = "m", nick = "n")
        val e = assertFailsWith<IllegalStateException> { props.requireComplete() }
        assertContains(e.message ?: fail("message absent"), "chorux.transport.xmpp.password")
        assertContains(e.message ?: fail("message absent"), "refusing to start")
    }

    @Test
    fun `disabled stays silent - honest single-node, no config demanded`() {
        XmppTransportProperties(enabled = false).requireComplete()
    }

    @Test
    fun `complete config passes the gate`() {
        XmppTransportProperties(enabled = true, host = "h", serviceName = "s", username = "u", password = "p", mucDomain = "m", nick = "n", resource = "r")
            .requireComplete()
    }
}
