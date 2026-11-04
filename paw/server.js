#!/usr/bin/env node
/*
 * The paw surface (docs/08) — an ADAPTER, an ordinary MCP client behind the
 * wall. Front speaks one-paw HTTP; behind, the MCP session dance happens in
 * <100 lines of zero-dependency client JS inside the page.
 *
 *   GET  /              -> the paw page (group/handle come from ?g=&as=)
 *   ANY  /mcp           -> streamed proxy to the room's MCP endpoint
 *
 * LISTEN is pulse-polling from the page (request/response): held-open
 * streams starve behind free tunnels — 2026-10-29, measured: POSTs passed,
 * zero SSE bytes. Payloads stay opaque end to end (L2); this file never
 * reads a say.
 */
"use strict";

const http = require("http");
const fs = require("fs");
const path = require("path");

const UPSTREAM = process.env.PAW_UPSTREAM || "http://localhost:8080";
const PORT = Number(process.env.PAW_PORT || 8090);
const CARD = path.join(__dirname, "paw.html");

const page = fs.readFileSync(CARD);

const server = http.createServer((req, res) => {
  const url = new URL(req.url, "http://paw.local");

  if (url.pathname === "/mcp") {
    // one-way relay: same method, same body, response relayed verbatim.
    // bounded: a paw's JSON-RPC never needs more than 64 KiB.
    const body = [];
    let size = 0;
    req.on("data", (c) => {
      size += c.length;
      if (size > 64 * 1024) {
        res.writeHead(413, { "content-type": "text/plain" });
        res.end("paws carry small payloads");
        req.destroy();
        return;
      }
      body.push(c);
    });
    req.on("end", () => {
      const upstream = http.request(
        UPSTREAM + "/mcp",
        {
          method: req.method,
          headers: {
            "content-type": "application/json",
            accept: req.headers.accept || "application/json, text/event-stream",
            ...(req.headers["mcp-session-id"]
              ? { "mcp-session-id": req.headers["mcp-session-id"] }
              : {}),
            "content-length": Buffer.concat(body).length,
          },
        },
        (up) => {
          res.writeHead(up.statusCode || 502, {
            "content-type": up.headers["content-type"] || "application/json",
            ...(up.headers["mcp-session-id"]
              ? { "mcp-session-id": up.headers["mcp-session-id"] }
              : {}),
            "cache-control": "no-store",
          });
          up.pipe(res);
        },
      );
      upstream.on("error", (e) => {
        res.writeHead(502, { "content-type": "text/plain" });
        res.end("room unreachable: " + e.message);
      });
      upstream.end(Buffer.concat(body));
    });
    return;
  }

  if (url.pathname === "/" || url.pathname === "/index.html") {
    res.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" });
    res.end(page);
    return;
  }

  res.writeHead(404, { "content-type": "text/plain" });
  res.end("paws only know / and /mcp");
});

server.listen(PORT, () => {
  console.log(`paw surface: http://localhost:${PORT}/ -> ${UPSTREAM}/mcp`);
});
