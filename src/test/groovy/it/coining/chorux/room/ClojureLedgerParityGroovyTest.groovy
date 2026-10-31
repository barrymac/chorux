package it.coining.chorux.room

import clojure.java.api.Clojure
import clojure.lang.Keyword
import clojure.lang.PersistentArrayMap
import it.coining.chorux.message.ChatEvent
import org.junit.jupiter.api.Test

/**
 * The Clojure reference ledger (src/main/clojure/chorux/ledger.clj)
 * must head the same chain as the Kotlin room for identical event sequences,
 * from nothing but the byte-exact canonical form pinned in docs/02.
 *
 * The Kotlin heads come from a real room driven through its public surface
 * (join/say/think — the operations that exist today); the events the room
 * realized are replayed as pure data through the Clojure chain. No reflection,
 * no clock mocking: both implementations consume the same realized values.
 */
class ClojureLedgerParityGroovyTest {

    private static Object cljVar(String ns, String name) {
        Clojure.var("clojure.core", "require").invoke(Clojure.read(ns))
        return Clojure.var(ns, name)
    }

    private static Map<Object, Object> dataOf(ChatEvent e) {
        Map<Object, Object> m = new LinkedHashMap<>()
        m.put(Keyword.intern("group"), e.group)
        m.put(Keyword.intern("handle"), e.handle)
        m.put(Keyword.intern("at"), e.at.toString())
        switch (e) {
            case ChatEvent.Said:
                m.put(Keyword.intern("type"), Keyword.intern("said"))
                m.put(Keyword.intern("text"), e.text)
                m.put(Keyword.intern("formats"), new ArrayList<>(e.formats))
                m.put(Keyword.intern("links"), new ArrayList<>(e.links))
                break
            case ChatEvent.Thought:
                m.put(Keyword.intern("type"), Keyword.intern("thought"))
                m.put(Keyword.intern("trace"), e.trace)
                break
            case ChatEvent.Joined:
                m.put(Keyword.intern("type"), Keyword.intern("joined"))
                break
            case ChatEvent.Left:
                m.put(Keyword.intern("type"), Keyword.intern("left"))
                break
            default:
                throw new IllegalArgumentException("unknown event: " + e)
        }
        return m
    }

    private static List<PersistentArrayMap> cljEvents(List<Map<Object, Object>> events) {
        return events.collect { PersistentArrayMap.create(it) }
    }

    @Test
    void "clojure reference ledger heads the same chain as the kotlin room"() {
        def room = new ChatRoom("parity", 100)
        room.join("ked")
        room.say("ked", 'say with [[ledger:proof]] and [[card:00000000-0000-4000-8000-000000000000]]', ["note", "trace"])
        room.say("ked", "pipes | in | text and unicode 中文 — canonicalization is adversarial", [])
        room.think("ked", "trace with | pipe and [[not-lifted:from-thoughts]]")
        def pulse = room.pulse()

        assert pulse.recent.size() == 4
        def cljHead = cljVar("chorux.ledger", "head").invoke(cljEvents(pulse.recent.collect { dataOf(it) }))
        assert pulse.ledgerHead != null
        assert cljHead == pulse.ledgerHead
    }

    @Test
    void "empty chain heads nil, per the spec"() {
        def cljHead = cljVar("chorux.ledger", "head").invoke([])
        assert cljHead == null
    }

    @Test
    void "canonical pins: reserved Left and the join forms, byte-exact"() {
        def canonical = cljVar("chorux.ledger", "canonical")
        def left = PersistentArrayMap.create([
            (Keyword.intern("type"))  : Keyword.intern("left"),
            (Keyword.intern("group")) : "g",
            (Keyword.intern("handle")): "h",
            (Keyword.intern("at"))    : "2026-10-29T00:00:00Z",
        ])
        assert canonical.invoke(left) == "Left|g|h|2026-10-29T00:00:00Z|"

        def said = PersistentArrayMap.create([
            (Keyword.intern("type"))   : Keyword.intern("said"),
            (Keyword.intern("group"))  : "g",
            (Keyword.intern("handle")) : "h",
            (Keyword.intern("at"))     : "T0",
            (Keyword.intern("text"))   : "txt",
            (Keyword.intern("formats")): ["a", "b"],
            (Keyword.intern("links"))  : ["u:v", "w:x"],
        ])
        assert canonical.invoke(said) == "Said|g|h|T0|txt|a,b|u:v,w:x"

        def saidBare = PersistentArrayMap.create([
            (Keyword.intern("type"))   : Keyword.intern("said"),
            (Keyword.intern("group"))  : "g",
            (Keyword.intern("handle")) : "h",
            (Keyword.intern("at"))     : "T0",
            (Keyword.intern("text"))   : "plain",
            (Keyword.intern("formats")): [],
            (Keyword.intern("links"))  : [],
        ])
        assert canonical.invoke(saidBare) == "Said|g|h|T0|plain||"

        def thought = PersistentArrayMap.create([
            (Keyword.intern("type"))  : Keyword.intern("thought"),
            (Keyword.intern("group")) : "g",
            (Keyword.intern("handle")): "h",
            (Keyword.intern("at"))    : "T0",
            (Keyword.intern("trace")) : "why | not",
        ])
        assert canonical.invoke(thought) == "Thought|g|h|T0|why | not"
    }
}
