import { execSync } from "node:child_process";
import path from "node:path";
import { describe, expect, it } from "vitest";

describe("wasm-pkg seam", () => {
  it("only wasm-module.ts reaches into web/src/wasm-pkg/", () => {
    const repoRoot = path.resolve(process.cwd(), "..");
    const raw = execSync(
      `git grep -l "wasm-pkg" web/src || true`,
      { cwd: repoRoot, encoding: "utf8", shell: "/bin/bash" },
    ).trim();
    const offenders = raw
      .split("\n")
      .filter(Boolean)
      .filter((file) => file !== "web/src/wasm-module.ts")
      .filter((file) => !file.startsWith("web/src/wasm-pkg/"))
      .filter((file) => !file.endsWith(".test.ts"));
    expect(offenders).toEqual([]);
  });
});
