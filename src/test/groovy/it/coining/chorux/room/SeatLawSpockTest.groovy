package it.coining.chorux.room

import spock.lang.Specification
import spock.lang.Subject
import spock.lang.Unroll

/**
 * The seat laws that catch people out — pinned as behaviour, in the shapes
 * that actually trapped us. Each feature documents the trap in its own name;
 * the data tables make the law's edges dense and readable.
 *
 * The traps, learned the hard way, live:
 *  - a seat looks like it belongs to a NAME, but it belongs to a CONNECTION:
 *    a proxy holding your handle locks YOU out
 *  - the room forgets released occupants — tenure does not survive the door:
 *    elders carry their tenure WITH them (docs/11), the room holds no history
 *  - a handle being present in the room is NOT permission to speak as it
 */
@Subject(ChatRoom)
class SeatLawSpockTest extends Specification {

    ChatRoom room = new ChatRoom("seat-laws", 100)

    def "a live claim refuses a second session — the claim belongs to the SESSION (the agent identity), not the handle"() {
        given: "an occupied seat"
        room.join("vip", "ses-holder")

        when: "a different connection claims the same name"
        room.join("vip", "ses-intruder")

        then: "refused loud, naming the law and the remedy"
        def ex = thrown(IllegalStateException)
        ex.message.contains("one live claim per handle")
        ex.message.contains("holder's disconnect releases it")
        and: "the original claim is untouched — the intruder squat changed nothing"
        room.say("vip", "ses-holder", "still mine")   // no exception: the holder still speaks
    }

    def "re-join by the SAME session is idempotent — reconnects are not double-joins"() {
        given: "a seated handle"
        def first = room.join("lane", "ses-a")

        when: "the same session joins its own handle again"
        def second = room.join("lane", "ses-a")

        then: "same occupant, one seat, no duplicate Joined event"
        second === first
        room.pulse().occupants.size() == 1
        room.pulse().recent.count { it instanceof it.coining.chorux.message.ChatEvent.Joined } == 1
    }

    def "the room forgets released seats — tenure does not survive the door (docs/11: elders carry their tenure)"() {
        given: "an elder seats and later releases (the connection ends)"
        def original = room.join("elder", "ses-old")
        def originalJoinedAt = original.joinedAt
        room.release("ses-old")

        when: "the elder returns through a new connection"
        def returned = room.join("elder", "ses-new")

        then: "the room remembers NOTHING of the visit — fresh seat, fresh clock"
        returned.joinedAt != originalJoinedAt
        room.pulse().occupants.size() == 1

        when: "the dead claim tries to speak"
        room.say("elder", "ses-old", "ghost say")

        then: "the door really closed"
        thrown(IllegalStateException)
    }

    def "connection end releases EVERY handle that connection held — one sweep, not N"() {
        given: "one connection holding three seats in one room"
        room.join("a", "ses-one")
        room.join("b", "ses-one")
        room.join("c", "ses-other")   // a different connection, for contrast

        when: "that connection ends"
        def released = room.release("ses-one")

        then: "exactly its handles released — a, not c"
        released.sort() == ["a", "b"]
        !room.pulse().occupants*.handle.contains("a")
        !room.pulse().occupants*.handle.contains("b")
        room.pulse().occupants*.handle.contains("c")
    }

    @Unroll
    def "say-as-held: a handle in the room is not a handle #who may speak as"() {
        given: "two members, one seat each"
        room.join("alice", "ses-alice")
        room.join("bob", "ses-bob")

        when:
        room.say(speaker, session, "spoof attempt")

        then:
        thrown(IllegalStateException)

        where:
        who          | speaker | session      | _
        "a stranger" | "alice" | "ses-stranger" | _
        "a peer"     | "alice" | "ses-bob"      | _
    }
}
