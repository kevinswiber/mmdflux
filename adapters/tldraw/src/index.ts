#!/usr/bin/env node

// Entry point: reads MMDS JSON from stdin, writes .tldr JSON to stdout.
//
// Usage:
//   mmdflux --format mmds diagram.mmd | node dist/index.js > out.tldr
//   mmdflux --format mmds --geometry-level routed diagram.mmd | node dist/index.js --open

import { execSync } from "node:child_process";
import { createServer } from "node:http";
import { parseArgs } from "node:util";
import type { MmdsDocument } from "@mmds/core";

import { convertToTldrawStore, toTldrawFile } from "./convert.js";

function readStdin(): Promise<string> {
  return new Promise((resolve, reject) => {
    let input = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (chunk: string) => {
      input += chunk;
    });
    process.stdin.on("end", () => resolve(input));
    process.stdin.on("error", reject);
  });
}

function openUrl(url: string) {
  const cmd =
    process.platform === "darwin"
      ? "open"
      : process.platform === "win32"
        ? "start"
        : "xdg-open";
  execSync(`${cmd} ${JSON.stringify(url)}`);
}

function previewHtml(): string {
  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>tldraw preview</title>
  <link rel="stylesheet" href="https://esm.sh/tldraw@4.4.0/tldraw.css">
  <script type="importmap">
    {
      "imports": {
        "react": "https://esm.sh/react@18.2.0",
        "react/jsx-runtime": "https://esm.sh/react@18.2.0/jsx-runtime",
        "react-dom": "https://esm.sh/react-dom@18.2.0",
        "react-dom/client": "https://esm.sh/react-dom@18.2.0/client",
        "tldraw": "https://esm.sh/tldraw@4.4.0?external=react,react-dom"
      }
    }
  </script>
</head>
<body>
  <div id="root" style="position:fixed;inset:0;"></div>
  <div id="error" style="display:none;padding:1rem;font-family:monospace;white-space:pre-wrap;color:#c00;"></div>
  <script type="module">
    window.process = window.process || { env: {} };
    const errEl = document.getElementById("error");
    const rootEl = document.getElementById("root");
    function showErr(msg) {
      rootEl.style.display = "none";
      errEl.style.display = "block";
      errEl.textContent = msg;
    }
    try {
      const r = await fetch("/diagram.json");
      if (!r.ok) throw new Error("Failed to fetch diagram: " + r.status);
      const file = await r.json();
      const { createRoot } = await import("react-dom/client");
      const React = await import("react");
      const { Tldraw, parseTldrawJsonFile, createTLStore } = await import("tldraw");
      const schema = createTLStore().schema;
      const result = parseTldrawJsonFile({ json: JSON.stringify(file), schema });
      if (!result.ok) {
        showErr("Parse failed: " + JSON.stringify(result.error, null, 2));
      } else {
        const store = result.value;
        const root = createRoot(rootEl);
        root.render(React.createElement(Tldraw, { store }));
      }
    } catch (e) {
      showErr("Error: " + (e?.message || e) + "\\n\\n" + (e?.stack || ""));
    }
  </script>
</body>
</html>`;
}

async function serveAndOpen(file: object) {
  const html = previewHtml();
  const diagramJson = JSON.stringify(file);
  const server = createServer((req, res) => {
    if (req.url === "/diagram.json") {
      res.writeHead(200, {
        "Content-Type": "application/json",
        "Content-Length": Buffer.byteLength(diagramJson, "utf8"),
      });
      res.end(diagramJson);
      return;
    }
    if (req.url === "/" || req.url === "") {
      res.writeHead(200, {
        "Content-Type": "text/html; charset=utf-8",
        "Content-Length": Buffer.byteLength(html, "utf8"),
      });
      res.end(html);
      return;
    }
    res.writeHead(404).end();
  });
  server.listen(0, "127.0.0.1", () => {
    const addr = server.address();
    if (addr && typeof addr === "object") {
      const url = `http://127.0.0.1:${addr.port}`;
      openUrl(url);
      console.error(`Preview at ${url} (Ctrl+C to stop)`);
    }
  });
}

async function main() {
  const { values } = parseArgs({
    options: {
      output: { type: "string", short: "o", default: "tldr" },
      scale: { type: "string", default: "1" },
      open: { type: "boolean", default: false },
    },
  });

  const output = values.output === "json" ? "json" : "tldr";
  const scale = Number(values.scale ?? "1");
  const shouldOpen = values.open ?? false;
  if (!Number.isFinite(scale) || scale <= 0) {
    console.error("--scale must be a positive finite number");
    process.exit(1);
  }

  let mmds: MmdsDocument;
  try {
    const raw = await readStdin();
    mmds = JSON.parse(raw);
  } catch (err) {
    console.error(
      `Invalid MMDS JSON on stdin: ${err instanceof Error ? err.message : err}`,
    );
    process.exit(1);
  }

  const store = convertToTldrawStore(mmds, { scale });
  const file = toTldrawFile(store);

  if (shouldOpen) {
    await serveAndOpen(file);
    return;
  }

  if (output === "json") {
    console.log(JSON.stringify(store, null, 2));
    process.exit(0);
  }

  console.log(JSON.stringify(file, null, 2));
  process.exit(0);
}

main();
