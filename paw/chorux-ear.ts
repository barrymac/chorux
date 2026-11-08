/**
 * chorux-ear.ts — a chorux room ear as a Pi extension (earendil-works/pi).
 *
 * Install: drop into ~/.pi/agent/extensions/ (or ~/.omo/agent/extensions/),
 * make ~/.pi/agent/chorux-ear.json (or a workspace-local .pi/chorux-ear.json):
 *   { "url": "http://<room-host>:8080", "group": "chorux", "handle": "omo",
 *     "everyMs": 5000 }
 *
 * What it does (docs/08 ARRIVE + LISTEN acts, pi-tier; docs/09 ear-gate law):
 *   - ARRIVE: calls join(group, handle) on handshake (reflexive formation, L5;
 *     self-heals across server restarts and seats the handle in occupants).
 *   - LISTEN: polls the room's pulse over plain request/response MCP — no
 *     streams, nothing held open — and when a new say rings this seat's gate
 *     (@<handle>, @all, @broadcast outside '>' quoted lines), injects it into
 *     THIS session via pi.sendMessage(..., { deliverAs: "steer", triggerTurn: true }).
 *
 * Cursor discipline: newest delivered event time is persisted per workspace
 * (~/.pi/agent/chorux-ear.<cwd-hash>.cursor, seeded from legacy
 * ~/.pi/agent/chorux-ear.cursor on first run) so multiple sessions sharing a
 * handle on one host never steal each other's cursor.
 *
 * Payloads stay opaque (L2): this extension never interprets text; it just
 * carries lines into the session. The agent decides what to do with them.
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const GLOBAL_CONF_PATH = join(homedir(), ".pi", "agent", "chorux-ear.json");
const LEGACY_CURSOR_PATH = join(homedir(), ".pi", "agent", "chorux-ear.cursor");

interface Conf { url: string; group: string; handle: string; everyMs: number; cursorPath: string }

function loadConf(cwd: string = process.cwd()): Conf {
  const localConfPath = join(cwd, ".pi", "chorux-ear.json");
  const confPath = existsSync(localConfPath) ? localConfPath : GLOBAL_CONF_PATH;
  const raw = existsSync(confPath) ? JSON.parse(readFileSync(confPath, "utf8")) : {};
  if (!raw.url) throw new Error(`chorux-ear: missing "url" in ${confPath} — fail loud, never guess a room`);
  const group = raw.group ?? "chorux";
  const handle = raw.handle ?? "omo";
  const scopeHash = createHash("sha256").update(`${cwd}\0${group}\0${handle}`).digest("hex").slice(0, 12);
  const cursorPath = join(homedir(), ".pi", "agent", `chorux-ear.${scopeHash}.cursor`);
  return {
    url: String(raw.url).replace(/\/$/, ""),
    group,
    handle,
    everyMs: raw.everyMs ?? 5000,
    cursorPath,
  };
}

/**
 * docs/09 ear-gate rules:
 *   1. self-skip (checked by caller on say.handle)
 *   2. quote non-amplification: strip '>' quoted lines before matching
 *   3. token match: @<handle>, @all, or @broadcast on token boundaries
 */
export function ringsGate(text: string, handle: string): boolean {
  const unquoted = text
    .split("\n")
    .filter(line => !line.trimStart().startsWith(">"))
    .join("\n");
  const escaped = handle.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const re = new RegExp(`(?:^|\\s)@(?:${escaped}|all|broadcast)(?=$|[\\s.,!?;:()\\[\\]{}"'—\\-])`);
  return re.test(unquoted);
}

class Mcp {
  session: string | null = null; rpcId = 0;
  constructor(private base: string) {}
  private async post(payload: any): Promise<any> {
    const res = await fetch(this.base + "/mcp", {
      method: "POST",
      headers: { "content-type": "application/json",
        accept: "application/json, text/event-stream",
        ...(this.session ? { "mcp-session-id": this.session } : {}) },
      body: JSON.stringify(payload),
    });
    const sid = res.headers.get("mcp-session-id");
    if (sid) this.session = sid;
    const raw = await res.text();
    for (const block of raw.split("\n\n")) {
      const data = block.split("\n").filter(l => l.startsWith("data:"))
        .map(l => l.slice(5).trim()).join("");
      if (!data) continue;
      try { const m = JSON.parse(data); if ("id" in payload && m.id === payload.id) return m; } catch {}
    }
    try { const m = JSON.parse(raw); if ("id" in payload && m.id === payload.id) return m; } catch {}
    return null;
  }
  async hello(group: string, handle: string) {
    this.session = null; // re-handshake after any failure
    const r = await this.post({ jsonrpc: "2.0", id: ++this.rpcId, method: "initialize",
      params: { protocolVersion: "2025-03-26", capabilities: {},
        clientInfo: { name: "pi-chorux-ear", version: "0.2.0" } } });
    if (!r) throw new Error("chorux-ear: initialize failed");
    await this.post({ jsonrpc: "2.0", method: "notifications/initialized" });
    // docs/08 ARRIVE act (L5 reflexive formation): ensures the group exists
    // after a server restart and seats our handle in pulse.occupants.
    await this.post({ jsonrpc: "2.0", id: ++this.rpcId, method: "tools/call",
      params: { name: "join", arguments: { group, handle } } });
  }
  async pulse(group: string): Promise<any> {
    const r = await this.post({ jsonrpc: "2.0", id: ++this.rpcId, method: "tools/call",
      params: { name: "pulse", arguments: { group } } });
    if (!r?.result?.content?.[0]?.text) throw new Error("chorux-ear: pulse failed");
    return JSON.parse(r.result.content[0].text);
  }
}

function readCursor(cursorPath: string): string {
  if (existsSync(cursorPath)) return readFileSync(cursorPath, "utf8").trim();
  if (existsSync(LEGACY_CURSOR_PATH)) return readFileSync(LEGACY_CURSOR_PATH, "utf8").trim();
  return "";
}

export default function (pi: ExtensionAPI) {
  const conf = loadConf();
  const mcp = new Mcp(conf.url);
  let ready = false;

  pi.on("session_start", async (_e: any, ctx: any) => {
    if (ready) return;
    ready = true;
    ctx.ui.setStatus("chorux-ear", `ear: ${conf.group}/@${conf.handle} @ ${conf.everyMs}ms`);

    const tick = async () => {
      try {
        const cursor = readCursor(conf.cursorPath);
        const p = await mcp.pulse(conf.group);
        let newest = cursor;
        for (const say of p.recent ?? []) {
          const at: string = say.at ?? "";
          if (at <= cursor) continue;
          if (at > newest) newest = at;
          const text: string = say.text ?? "";
          if (say.handle === conf.handle) continue; // self-skip (docs/09 rule 1): my own lines never ring my own bell
          if (!ringsGate(text, conf.handle)) continue;
          await pi.sendMessage(
            { customType: "chorux-ear",
              content: `[chorux/${say.handle} ${at.slice(11, 19)}] ${text}`,
              display: true },
            { deliverAs: "steer", triggerTurn: true });
        }
        if (newest !== cursor) writeFileSync(conf.cursorPath, newest);
      } catch (err) {
        // the ear must never kill the session — retry dance next interval
        try { await mcp.hello(conf.group, conf.handle); } catch { /* next breath */ }
      }
    };

    mcp.hello(conf.group, conf.handle).catch(() => {}).finally(() => {
      const timer = setInterval(tick, conf.everyMs);
      pi.on("session_shutdown", async () => { clearInterval(timer); }); // /reload must not stack zombie pollers
    });
  });
}
