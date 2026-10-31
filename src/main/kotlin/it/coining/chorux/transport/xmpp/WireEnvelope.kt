package it.coining.chorux.transport.xmpp

import it.coining.chorux.message.ChatEvent
import tools.jackson.databind.json.JsonMapper
import tools.jackson.module.kotlin.KotlinModule
import java.time.Instant

/**
 * The wire envelope (docs/07): a sealed event's docs/02 field data as one
 * plain JSON object. The ledger never travels — receivers chain their own
 * local records per docs/02. The codec never interprets payloads (L2):
 * it moves fields, nothing else.
 */
data class WireEnvelope(
    val event: String,
    val group: String,
    val handle: String,
    val at: Instant,
    val text: String? = null,
    val formats: List<String>? = null,
    val links: List<String>? = null,
    val trace: String? = null,
) {
    companion object {
        private val mapper = JsonMapper.builder()
            .addModule(KotlinModule.Builder().build())
            .build()

        /** Encode a sealed event as the wire JSON. `Left` never travels (docs/02, docs/07). */
        fun encode(e: ChatEvent): String? = when (e) {
            is ChatEvent.Said -> WireEnvelope("Said", e.group, e.handle, e.at, e.text, e.formats, e.links)
            is ChatEvent.Thought -> WireEnvelope("Thought", e.group, e.handle, e.at, trace = e.trace)
            is ChatEvent.Joined -> WireEnvelope("Joined", e.group, e.handle, e.at)
            is ChatEvent.Left -> null
        }.let { it?.let { w -> mapper.writeValueAsString(w) } }

        /**
         * Decode a groupchat body into the event it carries. Envelope JSON maps
         * back field-for-field; anything else is an ordinary chat line — the
         * external-client human half (L18) — and becomes a plain [ChatEvent.Said]
         * by the sender's nick. `Left` is refused (reserved, docs/02).
         */
        fun decode(body: String, group: String, fallbackHandle: String): ChatEvent = runCatching {
            val w = mapper.readValue(body, WireEnvelope::class.java)
            when (w.event) {
                "Said" -> ChatEvent.Said(w.group, w.handle, w.text.orEmpty(), w.formats.orEmpty(), w.links.orEmpty(), w.at)
                "Thought" -> ChatEvent.Thought(w.group, w.handle, w.trace.orEmpty(), w.at)
                "Joined" -> ChatEvent.Joined(w.group, w.handle, w.at)
                else -> throw IllegalArgumentException("unknown event: ${w.event}")
            }
        }.getOrElse { ChatEvent.Said(group, fallbackHandle, body, at = Instant.now()) }
    }
}
