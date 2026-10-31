package it.coining.chorux.harness

import it.coining.chorux.room.Pulse

/**
 * Prompt harnesses ARE architecture in chorux (operator directive 2026-10-28).
 *
 * A harness composes the prompt context an agent receives about a group: the
 * pulse, folded into whatever shape that agent's model reads best. Implementing
 * one is how a new seat (agent kind, human bridge, summarizer) enters the
 * chorus without changing the room.
 *
 * First implementations arrive with the first adoption wave — the interface is
 * published now so the shape is locked before behavior accretes.
 */
interface PromptHarness {
    /** Compose the context an agent named [handle] should see for [group]. */
    fun frame(group: String, handle: String, pulse: Pulse): String
}
