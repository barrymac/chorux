package it.coining.chorux.transport.xmpp

import it.coining.chorux.message.EventTransport
import it.coining.chorux.room.RoomRegistry
import org.springframework.boot.autoconfigure.condition.ConditionalOnProperty
import org.springframework.boot.context.properties.ConfigurationProperties
import org.springframework.boot.context.properties.EnableConfigurationProperties
import org.springframework.context.annotation.Bean
import org.springframework.context.annotation.Configuration

/**
 * chorux.transport.xmpp.* (docs/07) — fail-loud: with the transport enabled,
 * every missing value is named at once at startup; absent or disabled means
 * honest single-node, exactly as day one. No silent defaults, no wire facts
 * in any file that can reach a public tree.
 */
@ConfigurationProperties("chorux.transport.xmpp")
data class XmppTransportProperties(
    var enabled: Boolean = false,
    var host: String = "",
    var port: Int = 5222,
    var serviceName: String = "",
    var username: String = "",
    var password: String = "",
    var resource: String = "",
    var mucDomain: String = "",
    var nick: String = "",
    /** Optional CA pin (a CA cert or a bundle) for rails behind a private CA — absent = JVM default trust (docs/07). Two lawful forms: PEM CONTENT (a secret's natural shape, the env carries the cert itself) or a PATH to a PEM file; anything else refuses startup loud. */
    var trustCaPem: String = "",
) {
    /** The card's bar: refuse startup naming ALL missing values, never the first only. */
    fun requireComplete() {
        if (!enabled) return
        val missing = listOf(
            "chorux.transport.xmpp.host" to host,
            "chorux.transport.xmpp.service-name" to serviceName,
            "chorux.transport.xmpp.username" to username,
            "chorux.transport.xmpp.password" to password,
            "chorux.transport.xmpp.muc-domain" to mucDomain,
            "chorux.transport.xmpp.nick" to nick,
            "chorux.transport.xmpp.resource" to resource,
        ).filter { it.second.isBlank() }.map { it.first }
        check(missing.isEmpty()) {
            "chorux.transport.xmpp is enabled but missing: ${missing.joinToString(", ")} — refusing to start (fail-loud, docs/01 L11; no silent defaults)"
        }
    }
}

@Configuration
@EnableConfigurationProperties(XmppTransportProperties::class)
class XmppTransportWiring {

    @Bean
    @ConditionalOnProperty("chorux.transport.xmpp.enabled", havingValue = "true")
    fun xmppEventTransport(props: XmppTransportProperties, registry: RoomRegistry, transports: List<EventTransport>): XmppEventTransport {
        props.requireComplete()
        return XmppEventTransport(props, registry, transports)
    }
}
