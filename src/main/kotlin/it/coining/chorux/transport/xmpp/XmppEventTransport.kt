package it.coining.chorux.transport.xmpp

import it.coining.chorux.message.ChatEvent
import it.coining.chorux.message.EventTransport
import it.coining.chorux.room.RoomRegistry
import org.jivesoftware.smack.ReconnectionManager
import org.jivesoftware.smack.SmackException
import org.jivesoftware.smack.packet.Message
import org.jivesoftware.smack.packet.MessageBuilder
import org.jivesoftware.smack.tcp.XMPPTCPConnection
import org.jivesoftware.smack.tcp.XMPPTCPConnectionConfiguration
import org.jivesoftware.smackx.muc.MultiUserChat
import org.jivesoftware.smackx.muc.MultiUserChatManager
import org.jivesoftware.smackx.muc.ParticipantStatusListener
import org.jivesoftware.smackx.ping.PingManager
import org.jxmpp.jid.EntityBareJid
import org.jxmpp.jid.EntityFullJid
import org.jxmpp.jid.impl.JidCreate
import org.jxmpp.jid.parts.Resourcepart
import org.slf4j.LoggerFactory
import org.springframework.context.SmartLifecycle
import java.security.KeyStore
import java.security.cert.CertificateFactory
import java.security.cert.X509Certificate
import javax.net.ssl.TrustManagerFactory
import javax.net.ssl.X509TrustManager
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.LinkedBlockingDeque
import java.util.concurrent.TimeUnit

/**
 * The first real mechanism at the L11 seam (docs/07): one deployment is one
 * JID (the harness identity, L19); a chorux group maps to the MUC localpart
 * verbatim and forms reflexively both ways (L5). Emission is the member's
 * claim (L12) — delivery is proven by the receiver's pulse, never here.
 * Ingress is a member's act replayed through the room's own methods; the
 * ingest path never emits, and own-nick reflections are dropped, so an event
 * crosses the rail exactly once per instance.
 *
 * Tolerance (protocol law, decreed 2026-10-29 against the measured wedge):
 * the wall never waits on the glass. Emission queues to a single supervisor
 * worker (Dynamo's hinted handoff); the worker owns the connection's whole
 * lifecycle — boot dial, ping-health, drop-and-redial (Armstrong, let it
 * crash in small): a rail failure never kills the room, never blocks a say,
 * and is cured wholesale — a NEW connection object each time, never Smack's
 * auto-reconnect reviving a dead-threaded one (the 2026-10-29 fork-starved
 * reactor wedge was incurable from inside its own connection). One casualty
 * told honestly: an emit that crosses a redial may be sent twice on the
 * glass; the journal is the truth (L12), the mirror reconciles by pulse.
 */
class XmppEventTransport(
    private val props: XmppTransportProperties,
    private val registry: RoomRegistry,
    /** All seam taps except this one (Spring self-excludes the bean under creation): local sinks hear the room's whole timeline (docs/07). */
    private val transports: List<EventTransport> = emptyList(),
) : EventTransport, SmartLifecycle {

    companion object {
        /** Depth of the mirror's queue: an outage longer than this many events drops loud, never silently (L15). */
        const val EMIT_QUEUE_CAPACITY = 1000
        /** XEP-0199 cadence — the health oracle that declares a dead tap dead. */
        const val PING_SECONDS = 60
        /** Redial cadence while the rail is down. */
        const val REDIAL_SECONDS = 5L
    }

    private val log = LoggerFactory.getLogger(XmppEventTransport::class.java)
    private val lifecycleLock = Any()
    private var connection: XMPPTCPConnection? = null
    private val mucs = ConcurrentHashMap<String, MultiUserChat>()

    /** Set by the worker's dial and cleared by the ping oracle / any send failure. */
    @Volatile private var connected = false
    @Volatile private var running = false

    private data class PendingEmit(val event: ChatEvent, val body: String)

    private val queue = LinkedBlockingDeque<PendingEmit>(EMIT_QUEUE_CAPACITY)
    private val worker = Thread({ workLoop() }, "chorux-xmpp-mirror").apply { isDaemon = true }

    /** Boot tolerance: the room serves with the rail down; the worker dials in the background. Never throws. */
    override fun start() {
        synchronized(lifecycleLock) {
            if (running) return
            running = true
            worker.start()
        }
    }

    override fun stop() {
        running = false
        worker.interrupt()
        synchronized(lifecycleLock) { dropConnection() }
    }

    override fun isRunning(): Boolean = running
    override fun getPhase(): Int = Int.MAX_VALUE

    /**
     * The member's act mirrored out: NEVER blocks the request thread (L13 — the
     * wall is writable through any glass partition; the p99 is the wall's, not
     * the tap's). A full queue fails loud (L15), never silently.
     */
    override fun emit(event: ChatEvent) {
        val body = WireEnvelope.encode(event) ?: return
        if (!queue.offer(PendingEmit(event, body))) {
            log.error("xmpp mirror queue full ({} events) — dropping {} in {}", EMIT_QUEUE_CAPACITY, event::class.simpleName, event.group)
        }
    }

    /**
     * The supervisor loop: own the connection, incessantly. Retry a dead rail
     * every REDIAL_SECONDS forever; a send failure or the ping oracle condemns
     * the connection wholesale and the next cycle redials fresh.
     */
    private fun workLoop() {
        while (running) {
            val conn = try {
                ensureConnection()
            } catch (e: InterruptedException) {
                Thread.currentThread().interrupt(); return
            } catch (e: Exception) {
                log.error("xmpp rail not up ({}), redialing in {}s", e.toString(), REDIAL_SECONDS)
                if (!sleepInterruptibly(REDIAL_SECONDS)) return
                continue
            }
            val pending = try {
                queue.poll(REDIAL_SECONDS, TimeUnit.SECONDS) ?: continue
            } catch (e: InterruptedException) {
                Thread.currentThread().interrupt(); return
            }
            try {
                mucFor(pending.event.group, conn).sendMessage(buildMessage(pending.event.group, pending.body, conn))
            } catch (e: InterruptedException) {
                Thread.currentThread().interrupt(); return
            } catch (e: Exception) {
                log.error("xmpp mirror failed for {} in {}: {}", pending.event::class.simpleName, pending.event.group, e.toString())
                if (!queue.offerFirst(pending)) {
                    log.error("xmpp mirror queue full ({} events) — dropping {} in {}", EMIT_QUEUE_CAPACITY, pending.event::class.simpleName, pending.event.group)
                }
                dropConnection()
            }
        }
    }

    private fun sleepInterruptibly(seconds: Long): Boolean = try {
        Thread.sleep(seconds * 1000); true
    } catch (e: InterruptedException) {
        Thread.currentThread().interrupt(); false
    }

    /**
     * The supervisor's dial (Armstrong): serialized with every other lifecycle
     * act; reuses a live connection, otherwise builds a NEW one — Smack auto-
     * reconnect is explicitly refused: resurrecting a connection in-place keeps
     * its dead reader/reactor assignment; a fresh object rolls fresh dice and
     * clears every stale collector, listener and MUC handle with it.
     */
    private fun ensureConnection(): XMPPTCPConnection {
        synchronized(lifecycleLock) {
            connection?.takeIf { connected && it.isConnected }?.let { return it }
            dropConnection()
            val builder = XMPPTCPConnectionConfiguration.builder()
                .setHost(props.host)
                .setPort(props.port)
                .setXmppDomain(JidCreate.domainBareFrom(props.serviceName))
            pinnedTrustManager()?.let { tm ->
                builder.setCustomX509TrustManager(tm)
                builder.setHostnameVerifier { _, _ -> true }
            }
            val conn = XMPPTCPConnection(builder.build())
            ReconnectionManager.getInstanceFor(conn).disableAutomaticReconnection()
            conn.connect()
            conn.login(props.username, props.password, Resourcepart.from(props.resource))
            PingManager.getInstanceFor(conn).apply {
                setPingInterval(PING_SECONDS)
                registerPingFailedListener {
                    log.error("xmpp rail ping failed — the tap is declared dead; the supervisor redials")
                    connected = false
                }
            }
            connection = conn
            connected = true
            log.info("xmpp rail up: {}@{} as {}", props.username, props.serviceName, props.nick)
            return conn
        }
    }

    private fun dropConnection() {
        connected = false
        mucs.clear()
        connection?.let {
            try { ReconnectionManager.getInstanceFor(it).disableAutomaticReconnection() } catch (_: Exception) {}
            try { it.disconnect() } catch (_: Exception) {}
        }
        connection = null
    }

    private fun buildMessage(group: String, body: String, conn: XMPPTCPConnection): MessageBuilder =
        conn.stanzaFactory.buildMessageStanza()
            .to(roomJid(group))
            .ofType(Message.Type.groupchat)
            .setBody(body)

    private fun roomJid(group: String): EntityBareJid = JidCreate.entityBareFrom("$group@${props.mucDomain}")

    /**
     * The dialogue tier's occupant-addressed hop (docs/10): one Said, off-wall,
     * as a MUC private message to room@conference-domain/nick — the rail's own
     * createPrivateChat path (Smack 4.4.8, jar-verified). Identity stays bytes
     * for the nick exactly as for the group (docs/07): the address must survive
     * the rail's parser byte-exact or the call refuses loud. UNDELIVERABLE is
     * the only verdict for what this route cannot carry (blank nick, absent
     * occupant, rail down) — thrown to the caller, never logged-and-swallowed:
     * a direct reply the sender only believes sent is a claim, and this verb
     * returns state. Synchronous by decree: the caller buys the truth of the
     * hop with the wait, exactly unlike emit's queue.
     */
    override fun sayTo(event: ChatEvent.Said, nick: String) {
        if (nick.isBlank())
            throw IllegalArgumentException("UNDELIVERABLE: blank addressee nick (dialogue tier, docs/10)")
        val live = connection?.takeIf { connected && it.isConnected }
            ?: throw IllegalStateException("UNDELIVERABLE: xmpp transport not running (dialogue tier, docs/10)")
        val jid = occupantJid(event.group, nick)
        try {
            val muc = mucFor(event.group, live)
            if (muc.occupants.none { it.resourcepart.toString() == nick })
                throw IllegalArgumentException("UNDELIVERABLE: no occupant '$nick' in ${event.group} (dialogue tier, docs/10)")
            val body = WireEnvelope.encode(event)
                ?: throw IllegalArgumentException("UNDELIVERABLE: Left never travels (docs/02)")
            muc.createPrivateChat(jid, null).sendMessage(body)
        } catch (e: SmackException.NotConnectedException) {
            connected = false
            throw IllegalStateException("UNDELIVERABLE: rail not connected (dialogue tier, docs/10)", e)
        } catch (e: InterruptedException) {
            Thread.currentThread().interrupt()
            throw IllegalStateException("UNDELIVERABLE: interrupted mid-send (dialogue tier, docs/10)", e)
        }
    }

    /** docs/07 + docs/10: the occupant address is built through the rail's own parser and must roundtrip byte-exact — identity stays bytes, for nicks as for groups; anything else refuses loud. */
    internal fun occupantJid(group: String, nick: String): EntityFullJid {
        val full = JidCreate.entityFullFrom("$group@${props.mucDomain}/$nick")
        require(full.localpart.toString() == group && full.resourcepart.toString() == nick) {
            "UNDELIVERABLE: '$group/$nick' does not survive the rail's parser unchanged (identity stays bytes, docs/10)"
        }
        return full
    }

    /** Local sinks hear the room's whole timeline — a local tap cannot loop, and an ear that hears half the room is half an ear (docs/07). */
    internal fun tapLocal(event: ChatEvent) {
        transports.filter { it !== this }.forEach { tap ->
            try { tap.emit(event) } catch (_: Exception) {}
        }
    }

    /**
     * Private-CA rails (docs/07): the PEM pins trust at the seam. The CA pin IS the
     * trust boundary — chain validation runs against the pinned material, and the
     * leaf's name is not re-checked (private rails present unnamed leaves), so the
     * hostname gate opens only while the pin is set.
     */
    internal fun pinnedTrustManager(): X509TrustManager? {
        val pin = props.trustCaPem.trim()
        if (pin.isBlank()) return null
        // Two lawful forms (docs/07): PEM CONTENT — a kubernetes secret's natural
        // shape, the env carries the cert itself — or a PATH to a PEM file (a
        // mounted volume). A string that is neither fails loud at startup: an
        // unresolvable ref silently trusting nothing is worse than no boot.
        val source: java.io.InputStream = if (pin.startsWith("-----BEGIN")) {
            pin.byteInputStream()
        } else {
            val file = java.io.File(pin)
            require(file.isFile) {
                "chorux.transport.xmpp.trust-ca-pem is neither PEM content nor an existing file " +
                    "(starts '${pin.take(20).replace("\n", " ")}…') — fail-loud, docs/01"
            }
            file.inputStream()
        }
        return source.use { fis ->
            @Suppress("UNCHECKED_CAST")
            val certs: Collection<X509Certificate> = try {
                CertificateFactory.getInstance("X.509").generateCertificates(fis) as Collection<X509Certificate>
            } catch (e: java.security.cert.CertificateException) {
                emptyList()
            }
            require(certs.isNotEmpty()) {
                "chorux.transport.xmpp.trust-ca-pem holds no parseable certificate — refusing an empty trust anchor (fail-loud, docs/01)"
            }
            val ks = KeyStore.getInstance(KeyStore.getDefaultType())
            ks.load(null, null)
            certs.forEachIndexed { i, c -> ks.setCertificateEntry("chorux-trust-$i", c) }
            val tmf = TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm())
            tmf.init(ks)
            tmf.trustManagers.filterIsInstance<X509TrustManager>().first()
        }
    }

    private fun mucFor(group: String, conn: XMPPTCPConnection): MultiUserChat =
        mucs.computeIfAbsent(group) { joinMuc(group, conn) }

    /** Reflexive formation (L5): joining an absent MUC room forms it — and makes it instant, so the next arrival joins instead of meeting a locked half-created room. */
    private fun joinMuc(group: String, conn: XMPPTCPConnection): MultiUserChat {
        val muc = MultiUserChatManager.getInstanceFor(conn).getMultiUserChat(roomJid(group))
        muc.createOrJoin(Resourcepart.from(props.nick))?.makeInstant()
        muc.addMessageListener { stanza ->
            val from = stanza.from?.asEntityFullJidIfPossible()?.resourcepart?.toString().orEmpty()
            if (from.isNotBlank() && from != props.nick) ingest(WireEnvelope.decode(stanza.body ?: "", group, from), group, from)
        }
        muc.addParticipantStatusListener(object : ParticipantStatusListener {
            override fun joined(participant: org.jxmpp.jid.EntityFullJid) {
                val nick = participant.resourcepart.toString()
                if (nick != props.nick) registry.formOrJoin(group, nick)
            }
        })
        return muc
    }

    /** Ingress: someone's intentional act, replayed through the room's own methods. Never re-emits to the rail; local taps hear it (docs/07). */
    private fun ingest(event: ChatEvent, group: String, handle: String) {
        try {
            val room = registry.formOrJoin(group, handle).first
            when (event) {
                is ChatEvent.Said -> room.say(event.handle, event.text, event.formats)
                is ChatEvent.Thought -> room.think(event.handle, event.trace)
                is ChatEvent.Joined -> registry.formOrJoin(group, event.handle)
                is ChatEvent.Left -> Unit
            }
            tapLocal(event)
        } catch (e: Exception) {
            log.error("xmpp ingest failed in {}: {}", group, e.toString())
        }
    }
}
