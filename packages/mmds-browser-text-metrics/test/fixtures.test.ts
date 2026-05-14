import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  createAutoMmdsBrowserTextMetricsClient,
  createMmdsBrowserTextMetricsClient,
} from "../src/client.js";
import {
  createMockRenderWorker,
  environmentFixture,
  fontSetFixture,
  MockWorker,
  mainThreadEnvironmentFixture,
  multiStyleRequest,
  wasmModuleFixture,
} from "../src/fixtures.js";

const here = dirname(fileURLToPath(import.meta.url));

describe("fontSetFixture", () => {
  it("defaults to a resolved ready, load returning [{}], and check returning true", async () => {
    const fonts = fontSetFixture();
    expect(typeof fonts.load).toBe("function");
    expect(typeof fonts.check).toBe("function");
    expect(fonts.ready).toBeInstanceOf(Promise);
    await expect(fonts.ready).resolves.toBeUndefined();
    await expect(fonts.load("16px Inter")).resolves.toEqual([{}]);
    expect(fonts.check("16px Inter")).toBe(true);
  });

  it("honors overrides — ready never resolves when overridden with a hung promise", async () => {
    const hung = new Promise(() => {});
    const fonts = fontSetFixture({ ready: hung });
    expect(fonts.ready).toBe(hung);
    const settled = await Promise.race([
      fonts.ready,
      new Promise((resolveTimer) =>
        setTimeout(() => resolveTimer("timeout"), 10),
      ),
    ]);
    expect(settled).toBe("timeout");
  });

  it("spy records call args for assertions in downstream tests", async () => {
    const fonts = fontSetFixture();
    await fonts.load("12px Inter");
    await fonts.load("16px Verdana");
    expect(fonts.load.calls).toEqual([["12px Inter"], ["16px Verdana"]]);
  });
});

describe("environmentFixture + mainThreadEnvironmentFixture + multiStyleRequest", () => {
  it("environmentFixture exposes OffscreenCanvas + fonts; getContext('2d') returns the recording context", () => {
    const { context, environment } = environmentFixture();
    expect(environment.OffscreenCanvas).toBeDefined();
    expect(environment.fonts).toBeDefined();
    const canvas = new (
      environment.OffscreenCanvas as unknown as new (
        w: number,
        h: number,
      ) => { getContext: (t: string) => unknown }
    )(1, 1);
    expect(canvas.getContext("2d")).toBe(context);
    expect(canvas.getContext("webgl")).toBeNull();
  });

  it("mainThreadEnvironmentFixture exposes a document with fonts and createElement('canvas')", () => {
    const { context, environment } = mainThreadEnvironmentFixture();
    expect(environment.document?.fonts).toBeDefined();
    const canvas = environment.document?.createElement?.("canvas");
    expect(canvas).toBeDefined();
    expect(canvas?.getContext("2d")).toBe(context);
  });

  it("multiStyleRequest returns a two-style request with Inter + Verdana defaults", () => {
    const req = multiStyleRequest();
    expect(req.defaultStyle).toBe("s0");
    expect(req.textStyles).toHaveLength(2);
    expect(req.textStyles[0].fontFamily).toBe("Inter");
    expect(req.textStyles[1].fontFamily).toBe("Verdana");
  });
});

describe("createMockRenderWorker + MockWorker", () => {
  it("satisfies the Worker-like interface (postMessage, terminate, onmessage)", () => {
    const worker = createMockRenderWorker();
    expect(typeof worker.postMessage).toBe("function");
    expect(typeof worker.terminate).toBe("function");
    expect("onmessage" in worker).toBe(true);
  });

  it("MockWorker is exported as a class for callers that prefer instantiation", () => {
    const instance = new MockWorker();
    expect(typeof instance.postMessage).toBe("function");
    expect(typeof instance.terminate).toBe("function");
  });

  it("is directly assignable to createMmdsBrowserTextMetricsClient without a cast (compile-time)", () => {
    // No `as unknown as Worker` here — the client option is now MmdsWorkerLike,
    // a narrow surface every fixture and DOM Worker satisfies.
    const client = createMmdsBrowserTextMetricsClient({
      worker: createMockRenderWorker(),
    });
    expect(typeof client.terminate).toBe("function");
    client.terminate();
  });

  it("workerFactory: () => createMockRenderWorker() compiles for createAutoMmdsBrowserTextMetricsClient", () => {
    // Symmetric assignability: the auto-client's workerFactory accepts the
    // same MmdsWorkerLike contract, so fixtures work end-to-end without casts.
    const original = (globalThis as { Worker?: unknown }).Worker;
    (globalThis as { Worker?: unknown }).Worker = class {};
    try {
      const result = createAutoMmdsBrowserTextMetricsClient({
        workerFactory: () => createMockRenderWorker(),
      });
      expect(result).toHaveProperty("terminate");
      if ("terminate" in result && typeof result.terminate === "function") {
        result.terminate();
      }
    } finally {
      (globalThis as { Worker?: unknown }).Worker = original;
    }
  });
});

describe("wasmModuleFixture", () => {
  it("returns initialize, module, and per-method spies", async () => {
    const fixture = wasmModuleFixture();
    expect(typeof fixture.module.render).toBe("function");
    expect(typeof fixture.module.validate).toBe("function");
    expect(typeof fixture.module.browserTextMetricsRequest).toBe("function");
    expect(typeof fixture.module.renderWithBrowserTextMetrics).toBe("function");
    expect(typeof fixture.module.default).toBe("function");
    await fixture.module.default?.();
    expect(fixture.initialize.calls).toHaveLength(1);
  });
});

describe("fixtures.ts module boundary", () => {
  it("does not import vitest or node:test at runtime", () => {
    const source = readFileSync(resolve(here, "../src/fixtures.ts"), "utf8");
    expect(source).not.toMatch(/from\s+["']vitest["']/);
    expect(source).not.toMatch(/from\s+["']node:test["']/);
    expect(source).not.toMatch(/require\(["']vitest["']\)/);
  });

  it("dist/fixtures.js (post-build) has no vitest or node:test transitive import", () => {
    const distPath = resolve(here, "../dist/fixtures.js");
    let dist: string;
    try {
      dist = readFileSync(distPath, "utf8");
    } catch {
      // build artifact not produced yet; skip gracefully so the test isn't a flake
      return;
    }
    expect(dist).not.toMatch(/vitest/);
    expect(dist).not.toMatch(/node:test/);
  });
});
