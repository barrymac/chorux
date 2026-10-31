# AGENTS.md — chorux

Public repo. No internal hostnames, IP addresses, cluster names, secrets, or
proprietary references in ANY file, ever. Cite internal context generically.
Publish the laws, not the pain that taught them: results and doctrine ship
(laws, patterns, verdicts); the archaeology of how they were learned (grind
timelines, census counts, war stories) stays internal.

## Cardinal rules (the laws are docs/01; these are the working rules)

1. The tools surface is FOUR tools (`sayTo` joined by the L8 case in
   docs/10). A fifth tool → first write a case against L8 in a doc;
   never just add it.
2. Payloads stay opaque. Never add parsing/routing of `text` (L2). Marker
   lifting is regex-on-syntax only and must never infer semantics.
3. No unsolicited broadcasts. Core has no push path by design; keep it
   structurally true (there's a test for this; if you feel it itching, the
   itch is a client integration outside core).
4. Fail loud. Any config the system can't resolve = refused at startup with
   a clear error. No silent defaults, ever.
5. Small code. If a feature can live in a harness/client instead of core, it
   does. Core delegates heavy lifting to the transport seam (L11).
6. Joint language layout stays healthy: Kotlin main, Groovy welcome today,
   Clojure is the ledger's reference architecture.
7. Changes to the event envelope are protocol changes: docs/02 must move
   first, and the chain canonical form must not silently change (it
   re-heads history).
8. Observability attaches outboard (L16/L17): external subscribers or
   `EventTransport` seam taps. Never a metrics dialect in core, never
   required client instrumentation, never a named stack in protocol
   surfaces.
9. The rail is instant messaging: real clients on real transports only
   (XMPP C2S/MUC/MAM; MCP/SSE for the room). A human's client is never
   scraped, driven, or proxied as an agent interface. No script bridges
   where a transport exists; no files-as-protocol. When the rail can't
   carry something, build rail — never a workaround around it.

## Build & test

- `./gradlew test` — full suite (Kotlin + Groovy tests).
- `./gradlew bootRun` — run locally on :8080 (MCP streamable HTTP).
- Java 21 toolchain. Gradle wrapper is the only build tool needed.

## Layout

- `src/main/kotlin/it/coining/chorux/message/Protocol.kt` — the event
  envelope (the protocol IS here)
- `.../room/ChatRoom.kt` — the room, registry, ledger (L1, L5, L10)
- `.../surface/ChatRoomTools.kt` — the three MCP tools + nudges (L6–L8, L14)
- `.../surface/RoomEventStream.kt` — the listening half (L16): GET
  `/rooms/{group}/events.stream` SSE tap on the EventTransport seam, verbatim
  fan-out; mouth+ear = one duplex surface. Wake-gating lives downwind, never in it
- `.../surface/McpSurfaceWiring.kt` — Spring AI tool exposure
- `.../harness/PromptHarness.kt` — harness-as-architecture contract
- `.../eval/JudgementBattery.kt` + `gradlew jpEval` — pre-registered JP
  battery wire (`JP_BATTERY=<file> JEV_API_KEY=...`); batteries live OUTSIDE
  the exported tree (cards/ must stay unreachable in every exported ancestor)
- `docs/00–10` — genesis, laws, protocol, adoption, macro intents, attention
  vocabulary (doctrine), emergent collab protocol, paw-tier protocol, wake
  vocabulary, transport seam law, dialogue tier
- `SWARM.md` / `JOIN.md` — swarm onboarding (seats, ears, machine interface)
  and the one-paste agent join
- `src/main/clojure/chorux/ledger.clj` — Clojure reference ledger (L10).
  Data-as-code: ships in the jar as source, never loaded at runtime
- `src/test/{kotlin,groovy}` — law-pinning tests (incl. Clojure↔Kotlin ledger
  parity in `room/ClojureLedgerParityGroovyTest.groovy`)

## Public export recipe (github: barrymac/chorux)

github `main` is a squashed orphan export — full history stays on the gitea
origin. NEVER `git switch --orphan && git add -A` (that sweeps ignored junk —
build/, .gradle/, cards/ — into the export; happened 2026-10-29, repaired by
force-push). NEVER `git add -f` ignored paths either — the same sweep enters
through the front door (happened twice 2026-10-29: cards/ tracked on gitea,
exported, repaired by re-cutting the public lineage with zero card files
reachable in ANY ancestor). Cards RE-TRACKED on origin by operator order
2026-10-31 (board-as-git: mounts ingest the tracked worktree in place via
inotify — un-committed card moves still hold as git_divergent, keep the
commit-before-move law). Because origin's tree now contains cards/, the
export strips it via a temp index — zero workspace involvement. Only recipe:
`IDX=$(mktemp) && GIT_INDEX_FILE=$IDX git read-tree 'main^{tree}' &&
GIT_INDEX_FILE=$IDX git rm -q --cached -r cards && T=$(GIT_INDEX_FILE=$IDX
git write-tree) && rm -f $IDX && C=$(git commit-tree "$T" -m "…") &&
git push --force-with-lease github $C:main`. Pre-push guards:
(1) filename guard — `git ls-tree -r $C --name-only | grep -E '^cards/|HANDOFF-20|\.pem$|\.key$'`
MUST print nothing; with cards/ tracked on origin this now positively
verifies the strip — any ^cards/ hit means the temp-index step was skipped
or failed, stop. Grow this denylist with any new leak class (uuid shapes,
secret names) before the next export. (2) content guard — run the CI
security-scan pattern set (the `PATTERNS` heredoc in `.woodpecker.yml`, the
single source of truth for content leak classes) against the export
candidate: `sed -n "/<<'PATTERNS'/,/PATTERNS$/p" .woodpecker.yml | sed '1d;$d'
| while read -r P; do git grep -InE -e "$P" $C -- . ':!.woodpecker.yml'; done`
(exclude the pattern-list file, as CI does, or the gate matches itself) MUST print nothing
beyond sanctioned fixtures (synthetic marker/id strings in tests and docs —
a WORKING marker prefix or `00000000-…` id is functional, never a leak).
Verified 2026-10-30: the guard is the last gate and the
only one that sees the export tree whole; the filename guard alone let
HANDOFF-2026-10-30.md and a .pem ride the tip for a day. Export
messages are release-notes only — no card UUIDs, no process lore, no internal
identifiers.
