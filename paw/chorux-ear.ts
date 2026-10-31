/**
 * chorux-ear.ts — a chorux room ear as a Pi extension (earendil-works/pi).
 *
 * Install: drop into ~/.pi/agent/extensions/ , make ~/.pi/agent/chorux-ear.json:
 *   { "url": "http://<room-host>:8080", "group": "chorux", "handle": "omo",
 *     "everyMs": 5000 }
 *
 * What it does (docs/08 LISTEN act, pi-tier): polls the room's pulse over
 * plain request/response MCP — no streams, nothing held open — and when a
 * new say mentions @<handle>, injects it into THIS session via
 * pi.sendMessage(..., { triggerTurn: true }) so the line becomes part of
 * the conversation (custom_message, LLM-visible) and wakes an idle turn.
 *
 * Cursor discipline: newest delivered event time is persisted at
 * ~/.pi/agent/chorux-ear.cursor — catch-up on first run, never a repeat.
 *
 * Payloads stay opaque (L2): this extension never interprets text; it just
 * carries lines into the session. The agent decides what to do with them.
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const CONF_PATH = join(homedir(), ".pi", "agent", "chorux-ear.json");
const CURSOR_PATH = join(homedir(), ".pi", "agent", "chorux-ear.cursor");

interface Conf { url: string; group: string; handle: string; everyMs: number }

function loadConf(): Conf {
  const raw = existsSync(CONF_PATH) ? JSON.parse(readFileSync(CONF_PATH, "utf8")) : {};
  if (!raw.url) throw new Error(`chorux-ear: missing "url" in ${CONF_PATH} — fail loud, never guess a room`);
  return { url: String(raw.url).replace(/\/$/, ""), group: raw.group ?? "chorux",
           handle: raw.handle ?? "omo", everyMs: raw.everyMs ?? 5000 };
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
  async hello() {
    this.session = null; // re-handshake after any failure
    const r = await this.post({ jsonrpc: "2.0", id: ++this.rpcId, method: "initialize",
      params: { protocolVersion: "2025-03-26", capabilities: {},
        clientInfo: { name: "pi-chorux-ear", version: "0.1.0" } } });
    if (!r) throw new Error("chorux-ear: initialize failed");
    await this.post({ jsonrpc: "2.0", method: "notifications/initialized" });
  }
  async pulse(group: string): Promise<any> {
    const r = await this.post({ jsonrpc: "2.0", id: ++this.rpcId, method: "tools/call",
      params: { name: "pulse", arguments: { group } } });
    if (!r?.result?.content?.[0]?.text) throw new Error("chorux-ear: pulse failed");
    return JSON.parse(r.result.content[0].text);
  }
}

export default function (pi: ExtensionAPI) {
  const conf = loadConf();
  const mcp = new Mcp(conf.url);
  const bell = "@" + conf.handle;
  let ready = false;

  pi.on("session_start", async (_e: any, ctx: any) => {
    if (ready) return;
    ready = true;
    ctx.ui.setStatus("chorux-ear", `ear: ${conf.group}/@${conf.handle} @ ${conf.everyMs}ms`);

    const tick = async () => {
      try {
        const cursor = existsSync(CURSOR_PATH) ? readFileSync(CURSOR_PATH, "utf8").trim() : "";
        const p = await mcp.pulse(conf.group);
        let newest = cursor;
        for (const say of p.recent ?? []) {
          const at: string = say.at ?? "";
          if (at <= cursor) continue;
          if (at > newest) newest = at;
          const text: string = say.text ?? "";
          if (say.handle === conf.handle) continue; // self-skip (docs/09 rule 1): my own lines never ring my own bell
          if (!text.includes(bell)) continue;
          await pi.sendMessage(
            { customType: "chorux-ear",
              content: `[chorux/${say.handle} ${at.slice(11, 19)}] ${text}`,
              display: true },
            { triggerTurn: true });
        }
        if (newest !== cursor) writeFileSync(CURSOR_PATH, newest);
      } catch (err) {
        // the ear must never kill the session — retry dance next interval
        try { await mcp.hello(); } catch { /* next breath */ }
      }
    };

    mcp.hello().catch(() => {}).finally(() => {
      const timer = setInterval(tick, conf.everyMs);
      pi.on("session_shutdown", async () => { clearInterval(timer); }); // /reload must not stack zombie pollers
    });
  });
}
