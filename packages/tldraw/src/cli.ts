#!/usr/bin/env node

import { spawn } from "node:child_process";
import { realpathSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import type { MmdsDocument } from "@mmds/core";

import { convertToTldrawStore, toTldrawFile } from "./index.js";

const PREVIEW_URL = process.env.PREVIEW_URL ?? "http://localhost:5173";

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
        ? "cmd"
        : "xdg-open";
  const args = process.platform === "win32" ? ["/c", "start", "", url] : [url];
  const child = spawn(cmd, args, {
    detached: true,
    stdio: "ignore",
  });
  child.unref();
}

function isDirectExecution() {
  const entry = process.argv[1];
  if (entry === undefined) {
    return false;
  }

  try {
    return realpathSync(entry) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return path.resolve(entry) === fileURLToPath(import.meta.url);
  }
}

export async function main() {
  const { values } = parseArgs({
    options: {
      output: { type: "string", short: "o", default: "tldr" },
      scale: { type: "string", default: "1" },
      "node-spacing": { type: "string", default: undefined },
      open: { type: "boolean", default: false },
    },
  });

  const output =
    values.output === "json"
      ? "json"
      : values.output === "url"
        ? "url"
        : "tldr";
  const scale = Number(values.scale ?? "1");
  const nodeSpacing =
    values["node-spacing"] != null ? Number(values["node-spacing"]) : undefined;
  const shouldOpen = values.open ?? false;
  if (!Number.isFinite(scale) || scale <= 0) {
    console.error("--scale must be a positive finite number");
    process.exit(1);
  }
  if (
    nodeSpacing !== undefined &&
    (!Number.isFinite(nodeSpacing) || nodeSpacing <= 0)
  ) {
    console.error("--node-spacing must be a positive number");
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

  const store = convertToTldrawStore(mmds, {
    scale,
    ...(nodeSpacing !== undefined && { nodeSpacing }),
  });
  const file = toTldrawFile(store);

  if (output === "url" || shouldOpen) {
    const res = await fetch(`${PREVIEW_URL}/api/diagram`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(file),
    });
    if (!res.ok) {
      console.error(
        `Preview server not reachable (${res.status}). Run \`npm run preview\` in packages/tldraw first.`,
      );
      process.exit(1);
    }
    const { id } = (await res.json()) as { ok: boolean; id: string };
    const url = `${PREVIEW_URL}/?id=${encodeURIComponent(id)}`;

    if (output === "url") {
      console.log(url);
      process.exit(0);
    }
    openUrl(url);
    console.error(`Preview at ${url}`);
    process.exit(0);
  }

  if (output === "json") {
    console.log(JSON.stringify(store, null, 2));
    process.exit(0);
  }

  console.log(JSON.stringify(file, null, 2));
  process.exit(0);
}

if (isDirectExecution()) {
  void main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  });
}
