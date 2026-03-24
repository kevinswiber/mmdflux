#!/usr/bin/env node

import { execSync } from "node:child_process";
import { randomBytes, subtle } from "node:crypto";
import { realpathSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { deflateSync } from "node:zlib";
import type { MmdsDocument } from "@mmds/core";

import type { Bounds } from "./index.js";
import { convert } from "./index.js";

function computeAppState(bounds: Bounds) {
  const pad = 50;
  const contentW = bounds.maxX - bounds.minX + pad * 2;
  const contentH = bounds.maxY - bounds.minY + pad * 2;
  const cx = bounds.minX + (bounds.maxX - bounds.minX) / 2;
  const cy = bounds.minY + (bounds.maxY - bounds.minY) / 2;

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

function concatBuffers(...buffers: Uint8Array[]): Uint8Array {
  const totalData = buffers.reduce((acc, b) => acc + b.byteLength, 0);
  const out = new Uint8Array(4 + 4 * buffers.length + totalData);
  const dv = new DataView(out.buffer);
  let cursor = 0;
  dv.setUint32(cursor, 1);
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
  const key = await subtle.generateKey({ name: "AES-GCM", length: 128 }, true, [
    "encrypt",
    "decrypt",
  ]);
  const jwk = await subtle.exportKey("jwk", key);
  const keyString = jwk.k ?? "";
  if (!keyString) {
    throw new Error("Failed to extract encryption key");
  }

  const encoder = new TextEncoder();
  const contentsMetadata = encoder.encode(JSON.stringify(null));
  const dataBuffer = encoder.encode(json);
  const innerBuffer = concatBuffers(contentsMetadata, dataBuffer);

  const deflated = deflateSync(innerBuffer);

  const iv = randomBytes(12);
  const encrypted = new Uint8Array(
    await subtle.encrypt({ name: "AES-GCM", iv }, key, deflated),
  );

  const encodingMetadata = encoder.encode(
    JSON.stringify({
      version: 2,
      compression: "pako@1",
      encryption: "AES-GCM",
    }),
  );
  const payload = concatBuffers(encodingMetadata, iv, encrypted);

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
      output: { type: "string", short: "o", default: "json" },
      open: { type: "boolean", default: false },
    },
  });
  const outputFormat = values.output === "url" ? "url" : "json";
  const shouldOpen = values.open ?? false;

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

if (isDirectExecution()) {
  void main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  });
}
