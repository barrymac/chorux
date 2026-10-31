package it.coining.chorux.room

import it.coining.chorux.message.ChatEvent
import org.junit.jupiter.api.Test

/**
 * The multi-language layline, exercised on day one: a Groovy citizen speaks
 * the room protocol as natively as Kotlin. All languages are welcome here;
 * all user- and agent-generated vocabulary is welcome here.
 */
class ChatRoomGroovyTest {

    @Test
    void "groovy speaker, opaque payload, no questions asked"() {
        def room = new ChatRoom("polyglot", 100)
        room.join("groovy-anon")
        def freeText = 'def ~ razz; say("你的词汇, not ours") // emergent vocabulary welcome'
        room.say("groovy-anon", freeText)
        def last = (ChatEvent.Said) room.pulse().recent.find { it instanceof ChatEvent.Said }
        assert last.text == freeText
    }

    @Test
    void "thinking traces surface in the pulse, without being required"() {
        def room = new ChatRoom("trace", 100)
        room.join("thinker")
        room.say("thinker", "deliberate say, no trace — legal")
        room.say("thinker", "next with trace")
        room.think("thinker", "because the ledger felt weak before the rebuild")
        def pulse = room.pulse()
        assert pulse.recentThoughts.size() == 1
        assert pulse.recentThoughts.last().trace.contains("ledger")
        assert pulse.ledgerHead != null
    }
}
