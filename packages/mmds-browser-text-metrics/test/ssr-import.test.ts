import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const distDir = resolve(here, "../dist");

const distAvailable = existsSync(resolve(distDir, "index.js"));

async function importDist(name: string): Promise<Record<string, unknown>> {
  const url = pathToFileURL(resolve(distDir, name)).href;
  // Bypass Vite's transform pipeline by going through Node's loader. The
  // built dist must be importable from a vanilla Node process.
  return (await import(/* @vite-ignore */ url)) as Record<string, unknown>;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe.runIf(distAvailable)("SSR-safe subpath imports", () => {
  it("./index — exports the root surface without touching globals", async () => {
    vi.stubGlobal("Worker", undefined);
    vi.stubGlobal("document", undefined);
    vi.stubGlobal("self", undefined);
    const mod = await importDist("index.js");
    expect(mod.MmdsBrowserTextMetricsCapabilityError).toBeTypeOf("function");
    expect(mod.isMmdsBrowserTextMetricsCapabilityError).toBeTypeOf("function");
    expect(mod.MMDS_BROWSER_TEXT_METRICS_PROFILE_ID).toBe(
      "mmdflux-browser-canvas-v1",
    );
    expect(mod.MMDS_BROWSER_TEXT_METRICS_PROFILE_VERSION).toBe(1);
  });

  it("./client — imports cleanly without Worker/document; createDefaultMmdsWorker throws only when invoked", async () => {
    vi.stubGlobal("Worker", undefined);
    vi.stubGlobal("document", undefined);
    const mod = await importDist("client.js");
    expect(mod.createMmdsBrowserTextMetricsClient).toBeTypeOf("function");
    expect(mod.createDefaultMmdsWorker).toBeTypeOf("function");
    expect(mod.createAutoMmdsBrowserTextMetricsClient).toBeTypeOf("function");
    // Eager construction must throw because Worker is undefined, but the
    // import itself succeeded — that proves no top-level Worker eval.
    expect(() =>
      (mod.createDefaultMmdsWorker as () => unknown)(),
    ).toThrowError();
  });

  it("./main-thread — imports cleanly without document; construction is lazy", async () => {
    vi.stubGlobal("document", undefined);
    const mod = await importDist("main-thread.js");
    expect(mod.createMmdsMainThreadRenderer).toBeTypeOf("function");
    // Constructing the renderer must not throw — wasm + document only get
    // touched when renderSvg/renderStatic/validate runs.
    const renderer = (mod.createMmdsMainThreadRenderer as () => unknown)();
    expect(renderer).toBeTypeOf("object");
  });

  it("./worker — imports cleanly without self", async () => {
    vi.stubGlobal("self", undefined);
    const mod = await importDist("worker.js");
    expect(mod.createWorkerRequestHandler).toBeTypeOf("function");
    expect(mod.classifyWasmError).toBeTypeOf("function");
  });

  it("./wasm-classifier — pure module with no globals", async () => {
    vi.stubGlobal("Worker", undefined);
    vi.stubGlobal("document", undefined);
    vi.stubGlobal("self", undefined);
    const mod = await importDist("wasm-classifier.js");
    expect(mod.classifyWasmError).toBeTypeOf("function");
    expect(mod.runWasm).toBeTypeOf("function");
  });

  it("./worker-protocol — pure types + PROTOCOL_VERSION + isWorkerRequestMessage", async () => {
    const mod = await importDist("worker-protocol.js");
    expect(mod.PROTOCOL_VERSION).toBe(1);
    expect(mod.isWorkerRequestMessage).toBeTypeOf("function");
  });

  it("./loader — imports cleanly; @mmds/wasm not evaluated until loadMmdsWasm() is invoked", async () => {
    const mod = await importDist("loader.js");
    expect(mod.loadMmdsWasm).toBeTypeOf("function");
    expect(mod.assertMmdsWasmExports).toBeTypeOf("function");
    // If import had eagerly evaluated `@mmds/wasm`, the test process would
    // either crash or have a `@mmds/wasm` resolution error. Reaching this
    // assertion is proof of the lazy contract.
  });

  it("./routing — pure function, no globals", async () => {
    vi.stubGlobal("Worker", undefined);
    vi.stubGlobal("document", undefined);
    vi.stubGlobal("self", undefined);
    const mod = await importDist("routing.js");
    expect(mod.mayNeedBrowserTextMetrics).toBeTypeOf("function");
    const fn = mod.mayNeedBrowserTextMetrics as (o: {
      input: string;
      configJson?: string;
    }) => boolean;
    expect(fn({ input: "graph TD\nA-->B" })).toBe(false);
    expect(fn({ input: "graph TD\nA[X]\nstyle A font-family:Verdana" })).toBe(
      true,
    );
  });

  it("./fixtures — imports cleanly with no vitest dependency at runtime", async () => {
    const mod = await importDist("fixtures.js");
    expect(mod.fontSetFixture).toBeTypeOf("function");
    expect(mod.MockWorker).toBeTypeOf("function");
    expect(mod.createMockRenderWorker).toBeTypeOf("function");
    expect(mod.wasmModuleFixture).toBeTypeOf("function");
  });
});
