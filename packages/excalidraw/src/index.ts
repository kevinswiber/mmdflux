#!/usr/bin/env node

// Entry point: reads MMDS JSON from stdin, writes .excalidraw JSON to stdout.
//
// Usage:
//   mmdflux --format mmds diagram.mmd | node dist/index.js > out.excalidraw
//   mmdflux --format mmds --geometry-level routed diagram.mmd | node dist/index.js > out.excalidraw
//   mmdflux --format mmds --geometry-level routed diagram.mmd | node dist/index.js -o url
//   mmdflux --format mmds --geometry-level routed diagram.mmd | node dist/index.js --open

import { execSync } from "node:child_process";
import { randomBytes, subtle } from "node:crypto";
import { parseArgs } from "node:util";
import { deflateSync } from "node:zlib";
import type { MmdsDocument } from "@mmds/core";

import type { Bounds } from "./convert.js";
import { convert } from "./convert.js";

const { values: cliArgs } = parseArgs({
  options: {
    output: { type: "string", short: "o", default: "json" },
    open: { type: "boolean", default: false },
  },
});
const outputFormat = cliArgs.output === "url" ? "url" : "json";
const shouldOpen = cliArgs.open ?? false;

function computeAppState(bounds: Bounds) {
  const pad = 50;
  const contentW = bounds.maxX - bounds.minX + pad * 2;
  const contentH = bounds.maxY - bounds.minY + pad * 2;
  const cx = bounds.minX + (bounds.maxX - bounds.minX) / 2;
  const cy = bounds.minY + (bounds.maxY - bounds.minY) / 2;

  // Fit to a 1200x800 default viewport
  const viewW = 1200;
  const viewH = 800;
  const zoom = Math.min(viewW / contentW, viewH / contentH, 1);

  return {
    theme: "light" as const,
    viewBackgroundColor: "#ffffff",
    scrollX: viewW / 2 - cx * zoom,
    scrollY: viewH / 2 - cy * zoom,
    zoom: { value: zoom },
  };
}

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

// ---------------------------------------------------------------------------
// Excalidraw upload: encrypt + compress + upload to json.excalidraw.com
// ---------------------------------------------------------------------------

/** Concatenate buffers in excalidraw's wire format:
 *  [VERSION:uint32=1] [SIZE1:uint32] [DATA1] [SIZE2:uint32] [DATA2] ... */
function concatBuffers(...buffers: Uint8Array[]): Uint8Array {
  const totalData = buffers.reduce((acc, b) => acc + b.byteLength, 0);
  const out = new Uint8Array(4 + 4 * buffers.length + totalData);
  const dv = new DataView(out.buffer);
  let cursor = 0;
  dv.setUint32(cursor, 1); // version = 1
  cursor += 4;
  for (const buf of buffers) {
    dv.setUint32(cursor, buf.byteLength);
    cursor += 4;
    out.set(buf, cursor);
    cursor += buf.byteLength;
  }
  return out;
}

async function uploadToExcalidraw(json: string): Promise<string> {
  // 1. Generate AES-GCM 128-bit key
  const key = await subtle.generateKey({ name: "AES-GCM", length: 128 }, true, [
    "encrypt",
    "decrypt",
  ]);
  const jwk = await subtle.exportKey("jwk", key);
  const keyString = jwk.k ?? "";
  if (!keyString) {
    throw new Error("Failed to extract encryption key");
  }

  // 2. Build inner buffer: [contentsMetadata, dataBuffer]
  const encoder = new TextEncoder();
  const contentsMetadata = encoder.encode(JSON.stringify(null));
  const dataBuffer = encoder.encode(json);
  const innerBuffer = concatBuffers(contentsMetadata, dataBuffer);

  // 3. Deflate
  const deflated = deflateSync(innerBuffer);

  // 4. Encrypt with AES-GCM
  const iv = randomBytes(12);
  const encrypted = new Uint8Array(
    await subtle.encrypt({ name: "AES-GCM", iv }, key, deflated),
  );

  // 5. Build outer buffer: [encodingMetadata, iv, encryptedData]
  const encodingMetadata = encoder.encode(
    JSON.stringify({
      version: 2,
      compression: "pako@1",
      encryption: "AES-GCM",
    }),
  );
  const payload = concatBuffers(encodingMetadata, iv, encrypted);

  // 6. POST to excalidraw
  const resp = await fetch("https://json.excalidraw.com/api/v2/post/", {
    method: "POST",
    body: Buffer.from(payload),
  });
  if (!resp.ok) {
    throw new Error(
      `Excalidraw upload failed: ${resp.status} ${resp.statusText}`,
    );
  }
  const { id } = (await resp.json()) as { id: string };

  return `https://excalidraw.com/#json=${id},${keyString}`;
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

async function main() {
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

  const { elements, bounds } = convert(mmds);

  const output = {
    type: "excalidraw",
    version: 2,
    source: "mmdflux",
    elements,
    appState: computeAppState(bounds),
  };

  const jsonStr = JSON.stringify(output, null, 2);
  const needsUpload = outputFormat === "url" || shouldOpen;
  const url = needsUpload ? await uploadToExcalidraw(jsonStr) : null;

  if (outputFormat === "json") {
    console.log(jsonStr);
  }
  if (outputFormat === "url" && url) {
    console.log(url);
  }
  if (shouldOpen && url) {
    openUrl(url);
  }
}

main();
