#!/usr/bin/env node

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

async function main() {
  const { values } = parseArgs({
    options: {
      output: { type: "string", short: "o", default: "tldr" },
      scale: { type: "string", default: "1" },
    },
  });

  const output = values.output === "json" ? "json" : "tldr";
  const scale = Number(values.scale ?? "1");
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

  if (output === "json") {
    console.log(JSON.stringify(store, null, 2));
    process.exit(0);
  }

  const file = toTldrawFile(store);
  console.log(JSON.stringify(file, null, 2));
  process.exit(0);
}

main();
