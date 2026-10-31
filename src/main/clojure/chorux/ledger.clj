(ns chorux.ledger
  "Clojure reference implementation of the room hash-chain ledger (card
  L10). The ledger is a VALUE: pure functions over persistent
  data, nothing mutable, nothing ambient. Events arrive as plain maps:

    {:type :joined|:said|:left|:thought :group s :handle s :at <str Instant>
     :text s :formats [s ...] :links [s ...]   ; :said only
     :trace s}                                  ; :thought only

  The canonical form is pinned byte-exactly in docs/02. CHANGING IT
  RE-HEADS HISTORY — it is a protocol change and docs/02 moves first.
  This namespace carries no behavior the spec does not state."
  (:require [clojure.string :as str])
  (:import [java.security MessageDigest]
           [java.nio.charset StandardCharsets]))

(def ^:private type-name
  "Sealed four (docs/02): keyword -> Kotlin simpleName."
  {:joined "Joined"
   :said   "Said"
   :left   "Left"      ; reserved — silently leaving isn't an operation yet
   :thought "Thought"})

(defn- payload
  "payload(Said) = text|formats,join(,)|links,join(,); payload(Thought) =
  trace; payload(Joined|Left) = \"\". Empty lists join to the empty string."
  [e]
  (case (:type e)
    :said    (str (:text e) "|" (str/join "," (:formats e)) "|" (str/join "," (:links e)))
    :thought (:trace e)
             ""))

(defn canonical
  "canonical(e) = SimpleName|group|handle|at|payload(e) — one line,
  pipe-separated, no framing beyond the spec."
  ^String [e]
  (str (type-name (:type e)) "|" (:group e) "|" (:handle e) "|" (str (:at e)) "|" (payload e)))

(defn- sha256-hex
  "Lowercase hex of SHA-256 over the UTF-8 bytes. Java Formatter renders
  signed bytes as two hex chars (2^8 added for negative bytes) — same as
  the Kotlin side's \"%02x\".format(byte)."
  ^String [^String s]
  (let [digest (.digest (MessageDigest/getInstance "SHA-256")
                        (.getBytes s StandardCharsets/UTF_8))]
    (apply str (map #(format "%02x" %) digest))))

(defn head
  "head(∅) = nil. Each step: hex(SHA-256(UTF-8((prevHead ?: \"\") + canonical(e))))
  with NO separator between prevHead and canonical(e). Any past edit changes
  every later head — tamper-evident by construction (L10)."
  [events]
  (reduce (fn [prev e] (sha256-hex (str (or prev "") (canonical e))))
          nil
          events))
