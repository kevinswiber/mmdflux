import { describe, expect, it, vi } from "vitest";
import { MmdsBrowserTextMetricsCapabilityError } from "../src/capability.js";
import {
  type MainThreadBrowserTextMetricsEnvironment,
  prepareMainThreadTextMetrics,
} from "../src/prepare.js";

interface FakeCanvasContext {
  font: string;
  measureText: (text: string) => { width: number };
}

interface FakeFontFaceSet {
  load: (cssFont: string) => Promise<unknown[]>;
  ready?: Promise<unknown>;
  check: (cssFont: string) => boolean;
}

function fontSetFixture(
  overrides: Partial<FakeFontFaceSet> = {},
): FakeFontFaceSet {
  return {
    load: vi.fn(async () => [{}]),
    ready: Promise.resolve(),
    check: vi.fn(() => true),
    ...overrides,
  };
}

function mainThreadEnvironmentFixture(
  fonts: FakeFontFaceSet | undefined = fontSetFixture(),
) {
  const context: FakeCanvasContext = {
    font: "",
    measureText: vi.fn((text: string) => ({ width: text.length * 3 })),
  };
  const canvas = {
    getContext: (type: "2d"): FakeCanvasContext | null =>
      type === "2d" ? context : null,
  };
  const document = {
    fonts,
    createElement: vi.fn(function (
      this: unknown,
      tagName: "canvas",
    ): typeof canvas {
      if (this !== document) {
        throw new Error("createElement lost document receiver");
      }
      if (tagName !== "canvas") {
        throw new Error(`unexpected element: ${tagName}`);
      }
      return canvas;
    }),
  };
  const environment: MainThreadBrowserTextMetricsEnvironment = { document };
  return { context, canvas, document, environment };
}

describe("prepareMainThreadTextMetrics", () => {
  it("happy path — load + check ordered, metricsJson populated, measureText syncs", async () => {
    const calls: string[] = [];
    const fonts = fontSetFixture({
      load: vi.fn(async () => {
        calls.push("load");
        return [{}];
      }),
      check: vi.fn(() => {
        calls.push("check");
        return true;
      }),
      ready: new Promise(() => {}),
    });
    const { context, environment } = mainThreadEnvironmentFixture(fonts);
    const prepared = await prepareMainThreadTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });
    expect(calls).toEqual(["load", "check"]);
    const width = prepared.measureText("XY", '16px "Inter"');
    expect(width).toBe(6);
    expect(context.font).toBe('16px "Inter"');
    const parsed = JSON.parse(prepared.metricsJson);
    expect(parsed.profileId).toBe("mmdflux-browser-canvas-v1");
    expect(parsed.textStyles[0].cssFont).toBe('normal 400 16px "Inter"');
  });

  it("missing document.fonts throws main-thread-font-face-set-unavailable", async () => {
    const promise = prepareMainThreadTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment: { document: { createElement: vi.fn() } },
    });
    await expect(promise).rejects.toMatchObject({
      code: "main-thread-font-face-set-unavailable",
      fallbackEligible: false,
    });
    await expect(promise).rejects.toBeInstanceOf(
      MmdsBrowserTextMetricsCapabilityError,
    );
  });

  it("missing document.createElement throws main-thread-canvas-unavailable", async () => {
    const promise = prepareMainThreadTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment: { document: { fonts: fontSetFixture() } },
    });
    await expect(promise).rejects.toMatchObject({
      code: "main-thread-canvas-unavailable",
      fallbackEligible: false,
    });
  });

  it("createElement('canvas') returning null throws main-thread-canvas-unavailable", async () => {
    const promise = prepareMainThreadTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment: {
        document: {
          fonts: fontSetFixture(),
          createElement: vi.fn(() => null),
        },
      },
    });
    await expect(promise).rejects.toMatchObject({
      code: "main-thread-canvas-unavailable",
      fallbackEligible: false,
    });
  });

  it("null 2D context throws main-thread-canvas-2d-context-unavailable", async () => {
    const promise = prepareMainThreadTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment: {
        document: {
          fonts: fontSetFixture(),
          createElement: vi.fn(() => ({ getContext: () => null })),
        },
      },
    });
    await expect(promise).rejects.toMatchObject({
      code: "main-thread-canvas-2d-context-unavailable",
      fallbackEligible: false,
    });
  });

  it("post-load check=false throws font-load-check-failed (fallbackEligible: false)", async () => {
    const { environment } = mainThreadEnvironmentFixture(
      fontSetFixture({
        load: vi.fn(async () => [{}]),
        check: vi.fn(() => false),
      }),
    );
    const promise = prepareMainThreadTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });
    await expect(promise).rejects.toMatchObject({
      code: "font-load-check-failed",
      fallbackEligible: false,
      cssFont: 'normal 400 16px "Inter"',
    });
  });

  it("each prepared instance has its own memoization cache", async () => {
    const { context, environment } = mainThreadEnvironmentFixture();
    const measureSpy = vi.fn((text: string) => ({ width: text.length * 3 }));
    context.measureText = measureSpy;

    const first = await prepareMainThreadTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });
    const second = await prepareMainThreadTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });

    first.measureText("hello", "16px Inter");
    first.measureText("hello", "16px Inter");
    expect(measureSpy).toHaveBeenCalledTimes(1);
    // Second prepared instance does not share the first's cache.
    second.measureText("hello", "16px Inter");
    expect(measureSpy).toHaveBeenCalledTimes(2);
    second.measureText("hello", "16px Inter");
    expect(measureSpy).toHaveBeenCalledTimes(2);
  });
});

describe("prepareMainThreadTextMetrics module boundaries", () => {
  it("does not read globalThis.document at module top level", async () => {
    // If the module ever reads `globalThis.document` eagerly, this dynamic
    // import would throw under a node environment where document is absent.
    const original = (globalThis as { document?: unknown }).document;
    (globalThis as { document?: unknown }).document = undefined;
    try {
      await import("../src/prepare.js");
    } finally {
      (globalThis as { document?: unknown }).document = original;
    }
  });
});
