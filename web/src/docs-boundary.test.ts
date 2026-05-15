import { readFile } from "node:fs/promises";
import path from "node:path";
import { describe, expect, it } from "vitest";

describe("dynamic font docs", () => {
  it("describe the playground dynamic font boundary", async () => {
    const docs = await readFile(
      path.resolve(process.cwd(), "../docs/development/wasm.md"),
      "utf8",
    );

    expect(docs).toContain(
      "playground routes graph-family SVG font styles through browser text metrics",
    );
    expect(docs).toContain("Text, ASCII, and sequence remain unsupported");
    expect(docs).toContain(
      "rendering fails instead of measuring a fallback font",
    );
  });

  it("describe load+check preflight without awaiting FontFaceSet.ready", async () => {
    const docs = await readFile(
      path.resolve(process.cwd(), "../docs/development/wasm.md"),
      "utf8",
    );

    expect(docs).not.toMatch(/await[^.]*\bready\b/);
    expect(docs).toContain("intentionally does NOT await `FontFaceSet.ready`");
    expect(docs).toContain(
      "`load` plus post-load `check` is the requested-font contract",
    );
  });

  it("point at the @mmds/browser-text-metrics adapter package", async () => {
    const docs = await readFile(
      path.resolve(process.cwd(), "../docs/development/wasm.md"),
      "utf8",
    );

    expect(docs).toContain("@mmds/browser-text-metrics");
    expect(docs).toContain("createMmdsBrowserTextMetricsClient");
    expect(docs).toContain("createMmdsMainThreadRenderer");
    expect(docs).toContain("mmds-browser-text-metrics-v*");
  });
});
