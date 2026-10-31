package it.coining.chorux.surface

import it.coining.chorux.room.RoomRegistry
import org.junit.jupiter.api.Test
import org.springframework.test.web.servlet.MockMvc
import org.springframework.test.web.servlet.request.MockMvcRequestBuilders.get
import org.springframework.test.web.servlet.result.MockMvcResultMatchers.request
import org.springframework.test.web.servlet.result.MockMvcResultMatchers.status
import org.springframework.test.web.servlet.setup.MockMvcBuilders
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class RoomEventStreamTest {

    private fun rig(replayLimit: Int = 500, backlog: Int = 256): Triple<MockMvc, RoomEventStream, ChatRoomTools> {
        val registry = RoomRegistry(100)
        val stream = RoomEventStream(registry, replayLimit, backlog)
        val tools = ChatRoomTools(registry, listOf(stream))
        return Triple(MockMvcBuilders.standaloneSetup(stream).build(), stream, tools)
    }

    @Test
    fun `listening to a group that does not exist is a loud 404 — groups form on join, not on eavesdrop`() {
        val (mvc, _, _) = rig()
        mvc.perform(get("/rooms/no-such-room/events.stream"))
            .andExpect(status().isNotFound)
    }

    @Test
    fun `the stream forwards surface events verbatim — the listening half of the duplex`() {
        val (mvc, stream, tools) = rig()
        tools.join("open-space", "speaker")
        val result = mvc.perform(get("/rooms/open-space/events.stream"))
            .andExpect(request().asyncStarted())
            .andReturn()

        tools.say("open-space", "speaker", "hello listeners")
        stream.awaitDrain()

        val body = result.response.contentAsString
        assertTrue(body.contains("hello listeners"), "stream must carry the said text, got: $body")
        assertTrue(body.contains("event:Said"), "SSE frame must name the event type, got: $body")
        assertTrue(body.contains("\"handle\":\"speaker\""), "provenance must travel, got: $body")
    }

    @Test
    fun `a listener who leaves is removed from the fan-out — no cross-talk to the departed`() {
        val (_, stream, tools) = rig()
        tools.join("open-space", "speaker")

        val emitter = stream.stream("open-space", null)
        tools.say("open-space", "speaker", "before you left")
        emitter.complete()
        tools.say("open-space", "speaker", "after you left")
        stream.awaitDrain()
        val count = stream.subscriberCount("open-space")

        assertTrue(count == 0,
            "a completed emitter must be retired from the fan-out — count=$count")
    }

    @Test
    fun `a reconnecting listener presenting Last-Event-ID receives only what it missed, in order`() {
        val (mvc, stream, tools) = rig()
        tools.join("open-space", "speaker")
        mvc.perform(get("/rooms/open-space/events.stream"))
            .andExpect(request().asyncStarted())

        tools.say("open-space", "speaker", "heard-already")
        tools.say("open-space", "speaker", "missed-while-away")

        val resumed = mvc.perform(
            get("/rooms/open-space/events.stream").header("Last-Event-ID", "1")
        ).andExpect(request().asyncStarted()).andReturn()
        stream.awaitDrain()

        val body = resumed.response.contentAsString
        assertTrue(body.contains("missed-while-away"), "resume must replay missed frames, got: $body")
        assertFalse(body.contains("heard-already"), "resume must not replay acknowledged frames, got: $body")
        assertFalse(body.contains(":gap"), "in-ring resume is complete — no gap, got: $body")
    }

    @Test
    fun `a listener whose past scrolled out of the ring gets an honest gap, not fake completeness`() {
        val (mvc, stream, tools) = rig(replayLimit = 2)
        tools.join("open-space", "speaker")
        mvc.perform(get("/rooms/open-space/events.stream"))
            .andExpect(request().asyncStarted())

        tools.say("open-space", "speaker", "scrolled-away")
        tools.say("open-space", "speaker", "kept-one")
        tools.say("open-space", "speaker", "kept-two")

        val resumed = mvc.perform(
            get("/rooms/open-space/events.stream").header("Last-Event-ID", "1")
        ).andExpect(request().asyncStarted()).andReturn()
        stream.awaitDrain()

        val body = resumed.response.contentAsString
        assertTrue(body.contains(":gap"), "stale resume must admit the gap, got: $body")
        assertTrue(body.contains("kept-two"), "whatever the ring still holds must replay, got: $body")
        assertFalse(body.contains("scrolled-away"), "what the ring lost must not be faked, got: $body")
    }

    @Test
    fun `heartbeats keep the wire warm so no proxy mistakes silence for a dead stream`() {
        val (mvc, stream, tools) = rig()
        tools.join("open-space", "speaker")
        val result = mvc.perform(get("/rooms/open-space/events.stream"))
            .andExpect(request().asyncStarted())
            .andReturn()

        stream.heartbeat()
        stream.awaitDrain()

        assertTrue(result.response.contentAsString.contains(":hb"),
            "heartbeat comment must reach live listeners, got: ${result.response.contentAsString}")
        assertTrue(stream.subscriberCount("open-space") == 1)
    }

    @Test
    fun `a listener whose send fails is retired loud and the tap keeps serving the healthy`() {
        val (_, stream, tools) = rig()
        tools.join("open-space", "speaker")
        // completed before any frame: every queued send throws on the listener's own
        // thread — the send-death path, deterministically
        stream.stream("open-space", null).complete()
        // raw and un-completed: buffers every frame, cannot die in this harness — no
        // container lifecycle to race, the healthy stands by construction
        stream.stream("open-space", null)

        tools.say("open-space", "speaker", "only the healthy hear this")
        stream.awaitDrain()
        val count = stream.subscriberCount("open-space")
        assertTrue(count == 1,
            "the failed listener must be retired while the healthy stands — count=$count")

        // the tap kept sequencing through the retirement: no throw, ring intact
        tools.say("open-space", "speaker", "and keeps ringing")
        stream.awaitDrain()
        assertTrue(stream.subscriberCount("open-space") == 1,
            "the healthy keeps standing and the retired never returns")
    }
}
