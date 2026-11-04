package it.coining.chorux.surface

import it.coining.chorux.message.ChatEvent
import it.coining.chorux.message.EventTransport
import it.coining.chorux.room.RoomRegistry
import jakarta.annotation.PreDestroy
import org.slf4j.LoggerFactory
import org.springframework.beans.factory.annotation.Value
import org.springframework.http.HttpStatus
import org.springframework.http.MediaType
import org.springframework.scheduling.annotation.Scheduled
import org.springframework.stereotype.Component
import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.PathVariable
import org.springframework.web.bind.annotation.RequestHeader
import org.springframework.web.bind.annotation.RestController
import org.springframework.web.server.ResponseStatusException
import org.springframework.web.servlet.mvc.method.annotation.SseEmitter
import java.util.ArrayDeque
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * The listening half of the duplex client: one GET holds a live SSE stream
 * of the group's events; the three tools (same app) are the speaking half.
 * Mouth and ear ride one host, one port, one protocol.
 *
 * Attachment is lawful by construction: this is an [EventTransport], so the
 * fan-out happens at the surface's seam taps (docs/07 — the member's
 * intentional act), never inside the room. The room keeps its pinned
 * public surface — join/say/think/pulse, nothing that could push.
 *
 * The stream is content-agnostic like the room it mirrors: every event
 * forwards verbatim (L2). Wake-gating — mention filters, wake rights,
 * quoted-poke framing — is ear/agent business, downwind of this endpoint.
 *
 * Resilient by protocol: frames carry SSE `id:` (per-group sequence), and a
 * reconnecting listener may present `Last-Event-ID` — anything still in the
 * replay ring is re-sent, in order, before live frames resume. A listener
 * whose last id has scrolled out of the ring (or whose group idled away)
 * gets an honest `:gap` comment — silence is never replayed as if nothing
 * was missed; catching up further is what pulse is for. Server-rendered
 * `:hb` comments keep the wire warm so tunnels and proxies can't silently
 * strangle an idle-looking stream.
 *
 * Queue-fed end to end (the tolerance law, 2026-10-29): the room's threads —
 * the HTTP tool thread on a member's act and Smack's reader via ingest ->
 * tapLocal — NEVER wait on a listener. Every frame, replay and heartbeat
 * alike, is offered (non-blocking) to a per-listener bounded backlog under
 * the group monitor, and sent by that listener's own daemon thread — so
 * per-listener FIFO order equals ring order, a stalled socket can only
 * ever retire its own listener, and no send of any kind sits on a caller's
 * thread or under the monitor.
 *
 * Fail-loud: listening to a group that does not exist is a 404, never a
 * silent formation (groups form on join, not on eavesdrop).
 */
@RestController
@Component
class RoomEventStream(
    private val registry: RoomRegistry,
    @Value("\${chorux.stream.replay-limit:500}") private val replayLimit: Int,
    /** Per-listener bound of frames awaiting one listener's socket — the slow-consumer
     *  fence. Must cover a full-ring resume: default 512 > default replay-limit 500. */
    @Value("\${chorux.stream.emitter-backlog:512}") private val emitterBacklog: Int,
) : EventTransport {

    private val log = LoggerFactory.getLogger(RoomEventStream::class.java)

    private data class Sequenced(val id: Long, val name: String, val data: Map<String, Any?>)

    /** One unit of listener work: a live frame, or a drain barrier carrying its own latch. */
    private sealed interface Frame {
        data class Live(val sse: SseEmitter.SseEventBuilder) : Frame
        data class Barrier(val latch: CountDownLatch) : Frame
    }

    /**
     * A listener: its emitter, its bounded backlog, and the daemon thread
     * that drains both. Isolation is the point — one listener's stalled
     * socket can only ever wedge itself; retirement (overflow, send death,
     * lifecycle callback, shutdown) is loud, single-shot, and its own cure
     * is the protocol's: reconnect with Last-Event-ID, meet the :gap.
     */
    private class Listener(
        val group: String,
        val emitter: SseEmitter,
        backlog: Int,
        private val onRetire: (Listener, String) -> Unit,
    ) : Runnable {
        val queue = ArrayBlockingQueue<Frame>(backlog)
        @Volatile var retired = false
        val thread = Thread(this, "sse-tap-$group").apply { isDaemon = true }

        fun retire(why: String) {
            if (retired) return
            retired = true
            try { emitter.complete() } catch (_: Exception) {}
            thread.interrupt()
            onRetire(this, why)
        }

        override fun run() {
            while (!retired) {
                val f = try { queue.take() } catch (e: InterruptedException) { return }
                when (f) {
                    is Frame.Barrier -> f.latch.countDown()
                    is Frame.Live -> try {
                        emitter.send(f.sse)
                    } catch (e: Exception) {
                        // IO death AND lifecycle death both mean the listener is
                        // gone — retire loud; callbacks may not fire outside
                        // container async, so this loop is its own cleanup.
                        retire("send failed: ${e::class.simpleName}: ${e.message}")
                        return
                    }
                }
            }
        }
    }

    private class GroupStream {
        val listeners = ConcurrentHashMap.newKeySet<Listener>()
        val ring = ArrayDeque<Sequenced>()
        var seq = 0L
    }

    /** Live group streams; an entry exists only while it has listeners. */
    private val groups = ConcurrentHashMap<String, GroupStream>()

    @GetMapping("/rooms/{group}/events.stream", produces = [MediaType.TEXT_EVENT_STREAM_VALUE])
    fun stream(
        @PathVariable group: String,
        @RequestHeader("Last-Event-ID", required = false) lastEventId: Long?,
    ): SseEmitter {
        registry.room(group)
            ?: throw ResponseStatusException(HttpStatus.NOT_FOUND,
                "no room '$group' — groups form on join, not on eavesdrop")

        val emitter = SseEmitter(0L)
        var fresh = false
        val gs = groups.computeIfAbsent(group) { fresh = true; GroupStream() }
        val listener = Listener(group, emitter, emitterBacklog) { l, why ->
            log.warn("sse listener retired in {}: {} — Last-Event-ID resume + :gap honesty is its cure", group, why)
            val g = groups[group]
            if (g != null) {
                synchronized(g) { g.listeners.remove(l) }
                if (g.listeners.isEmpty()) groups.remove(group, g)
            }
        }

        // Honest resume, queue-fed: the :gap comment and the ring's surviving
        // tail ride the SAME bounded FIFO as live frames, offered under the
        // group monitor, so a resume can never reorder against a concurrent
        // emit — and the request thread never sends at all.
        synchronized(gs) {
            gs.listeners += listener
            if (lastEventId != null) {
                if (fresh || (gs.ring.isNotEmpty() && lastEventId < gs.ring.first().id)) {
                    offer(listener, SseEmitter.event().comment("gap"), "resume :gap")
                }
                gs.ring.filter { it.id > lastEventId }
                    .forEach { offer(listener, SseEmitter.event().id(it.id.toString()).name(it.name).data(it.data), "resume replay") }
            }
        }
        listener.thread.start()

        emitter.onCompletion { listener.retire("listener completed") }
        emitter.onTimeout { listener.retire("listener timeout") }
        emitter.onError { listener.retire("listener error") }

        return emitter
    }

    /** Seam tap (docs/07): the surface emits; we sequence, ring and forward.
     *  Queue-fed: this NEVER sends inline — frames are offered to bounded
     *  per-listener backlogs under the group monitor (offer never blocks, so
     *  the monitor is held for microseconds, never across a send) and sent by
     *  listener threads, so one stalled consumer can only ever retire itself.
     *  A sink must NEVER throw through the tap — the pinned door stands:
     *  failures retire the listener loud and never break the member's act. */
    override fun emit(event: ChatEvent) {
        try {
            val gs = groups[event.group] ?: return
            synchronized(gs) {
                val frame = Sequenced(++gs.seq, event::class.simpleName ?: "Event", wire(event))
                gs.ring.addLast(frame)
                while (gs.ring.size > replayLimit) gs.ring.removeFirst()
                val sse = SseEmitter.event().id(frame.id.toString()).name(frame.name).data(frame.data)
                gs.listeners.toList().forEach { l -> offer(l, sse, "live frame") }
            }
        } catch (e: Exception) {
            log.error("sse tap emit failed in {}: {}", event.group, e.toString())
        }
    }

    /** Keepalive comment so no proxy or tunnel mistakes silence for a dead
     *  stream; also the vacuum for group entries whose listeners all left.
     *  Rides the same bounded queues — the heartbeat never waits on a socket. */
    @Scheduled(fixedRateString = "\${chorux.stream.heartbeat-ms:15000}")
    fun heartbeat() {
        groups.forEach { (_, gs) ->
            synchronized(gs) {
                gs.listeners.toList().forEach { l -> offer(l, SseEmitter.event().comment("hb"), "heartbeat") }
            }
        }
    }

    /** Offer one frame to one listener, never blocking: a full backlog means the
     *  consumer is slower than its catch-up — retire it loud, drop the frame,
     *  and let the protocol's resume be the cure. Never throws through the tap. */
    private fun offer(l: Listener, sse: SseEmitter.SseEventBuilder, what: String) {
        if (l.retired) return
        if (!l.queue.offer(Frame.Live(sse)))
            l.retire("backlog overflow ($emitterBacklog frames) at $what — slow consumer")
    }

    /**
     * Test-visible drain point: returns once every live listener has
     * processed a barrier queued after all frames offered before this call
     * — those frames are sent (or the listener has retired) by then. A
     * listener whose backlog is already full is stalled by definition and
     * is skipped; a listener that retires concurrently is bounded, not hung.
     */
    internal fun awaitDrain() {
        val live = groups.values.flatMap { gs -> synchronized(gs) { gs.listeners.toList() } }
        live.forEach { l ->
            val latch = CountDownLatch(1)
            if (!l.retired && l.queue.offer(Frame.Barrier(latch)))
                latch.await(2, TimeUnit.SECONDS)
        }
    }

    @PreDestroy
    fun shutdown() {
        groups.values.flatMap { gs -> synchronized(gs) { gs.listeners.toList() } }
            .forEach { it.retire("tap shutdown") }
    }

    /** Test-visible fan-out census — a retiring listener is retired the moment it says so:
     *  the census reads the flag, never the bookkeeping's set-removal. */
    internal fun subscriberCount(group: String): Int = groups[group]?.listeners?.count { !it.retired } ?: 0

    /** Verbatim event, one JSON object per SSE data frame. No interpretation. */
    private fun wire(e: ChatEvent): Map<String, Any?> = when (e) {
        is ChatEvent.Said -> mapOf(
            "type" to "Said", "group" to e.group, "handle" to e.handle, "at" to e.at.toString(),
            "text" to e.text, "formats" to e.formats, "links" to e.links,
        )
        is ChatEvent.Thought -> mapOf(
            "type" to "Thought", "group" to e.group, "handle" to e.handle, "at" to e.at.toString(),
            "trace" to e.trace,
        )
        is ChatEvent.Joined -> mapOf("type" to "Joined", "group" to e.group, "handle" to e.handle, "at" to e.at.toString())
        is ChatEvent.Left -> mapOf("type" to "Left", "group" to e.group, "handle" to e.handle, "at" to e.at.toString())
    }
}
