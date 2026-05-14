import { describe, expect, it, vi } from "vitest";
import { MmdsBrowserTextMetricsCapabilityError } from "../src/capability.js";
import {
  type BrowserTextMetricsEnvironment,
  prepareWorkerTextMetrics,
} from "../src/prepare.js";

interface FakeTextMetrics {
  width: number;
}

interface FakeCanvasContext {
  font: string;
  measureText: (text: string) => FakeTextMetrics;
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

function environmentFixture(
  fonts: FakeFontFaceSet | undefined = fontSetFixture(),
) {
  const context: FakeCanvasContext = {
    font: "",
    measureText: vi.fn((text: string) => ({ width: text.length * 3 })),
  };
  class FakeOffscreenCanvas {
    getContext(type: string): FakeCanvasContext | null {
      return type === "2d" ? context : null;
    }
  }
  const environment: BrowserTextMetricsEnvironment = {
    OffscreenCanvas: FakeOffscreenCanvas,
    fonts,
  };
  return { context, environment };
}

function multiStyleRequest() {
  return {
    defaultStyle: "s0",
    textStyles: [
      {
        id: "s0",
        fontFamily: "Inter",
        fontSize: 16,
        lineHeight: 24,
        fontStyle: "normal",
        fontWeight: "400",
      },
      {
        id: "s1",
        fontFamily: "Verdana",
        fontSize: 8,
        lineHeight: 12,
        fontStyle: "normal",
        fontWeight: "400",
      },
    ],
  };
}

describe("prepareWorkerTextMetrics", () => {
  it("Q6 §3.3a — missing OffscreenCanvas throws worker-offscreen-canvas-unavailable", async () => {
    await expect(
      prepareWorkerTextMetrics({
        request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
        environment: { fonts: fontSetFixture() },
      }),
    ).rejects.toMatchObject({
      code: "worker-offscreen-canvas-unavailable",
      fallbackEligible: true,
    });
    await expect(
      prepareWorkerTextMetrics({
        request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
        environment: { fonts: fontSetFixture() },
      }),
    ).rejects.toBeInstanceOf(MmdsBrowserTextMetricsCapabilityError);
  });

  it("Q6 §3.3b — missing fonts throws worker-font-face-set-unavailable", async () => {
    const { environment } = environmentFixture();
    environment.fonts = undefined;
    await expect(
      prepareWorkerTextMetrics({
        request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
        environment,
      }),
    ).rejects.toMatchObject({
      code: "worker-font-face-set-unavailable",
      fallbackEligible: true,
    });
  });

  it("Q6 §3.3c — null 2D context throws worker-canvas-2d-context-unavailable", async () => {
    class FakeOffscreenCanvasWithout2d {
      getContext(): null {
        return null;
      }
    }
    await expect(
      prepareWorkerTextMetrics({
        request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
        environment: {
          OffscreenCanvas: FakeOffscreenCanvasWithout2d,
          fonts: fontSetFixture(),
        },
      }),
    ).rejects.toMatchObject({
      code: "worker-canvas-2d-context-unavailable",
      fallbackEligible: true,
    });
  });

  it("Q6 §3.1 — calls load then check; never awaits ready", async () => {
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
      // ready that never resolves — proves it's not awaited
      ready: new Promise(() => {}),
    });
    const { environment } = environmentFixture(fonts);
    const prepared = await prepareWorkerTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });
    expect(calls).toEqual(["load", "check"]);
    expect(prepared.metricsJson).toContain('"cssFont"');
  });

  it("Q6 §3.11 — metricsJson exposes profileId, profileVersion, defaultStyle, cssFont", async () => {
    const { environment } = environmentFixture();
    const prepared = await prepareWorkerTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });
    const parsed = JSON.parse(prepared.metricsJson);
    expect(parsed.profileId).toBe("mmdflux-browser-canvas-v1");
    expect(parsed.profileVersion).toBe(1);
    expect(parsed.defaultStyle).toBe("s0");
    expect(parsed.textStyles[0].cssFont).toBe('normal 400 16px "Inter"');
  });

  it("Q6 §3.12 — empty load result with check=true succeeds (system fonts)", async () => {
    const { environment } = environmentFixture(
      fontSetFixture({
        load: vi.fn(async () => []),
        check: vi.fn(() => true),
      }),
    );
    await expect(
      prepareWorkerTextMetrics({
        request: { fontFamily: "system-ui", fontSizePx: 16, lineHeightPx: 24 },
        environment,
      }),
    ).resolves.toMatchObject({
      metricsJson: expect.any(String),
      measureText: expect.any(Function),
    });
  });

  it("Q6 §3.3e — post-load check=false throws font-load-check-failed (fallbackEligible: false)", async () => {
    const { environment } = environmentFixture(
      fontSetFixture({
        load: vi.fn(async () => [{}]),
        check: vi.fn(() => false),
      }),
    );
    const promise = prepareWorkerTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });
    await expect(promise).rejects.toMatchObject({
      code: "font-load-check-failed",
      fallbackEligible: false,
      cssFont: 'normal 400 16px "Inter"',
    });
    await expect(promise).rejects.toBeInstanceOf(
      MmdsBrowserTextMetricsCapabilityError,
    );
    await expect(promise).rejects.toThrow(/Inter/);
  });

  it("measureText sets context.font and returns finite width; -1 or NaN throws font-load-check-failed", async () => {
    const { context, environment } = environmentFixture();
    const prepared = await prepareWorkerTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });
    const width = prepared.measureText("AB", '16px "Inter"');
    expect(width).toBe(6);
    expect(context.font).toBe('16px "Inter"');

    context.measureText = vi.fn(() => ({ width: -1 }));
    expect(() => prepared.measureText("neg", '16px "Inter-x"')).toThrow(
      MmdsBrowserTextMetricsCapabilityError,
    );
    try {
      prepared.measureText("neg-again", '16px "Inter-y"');
    } catch (err) {
      expect(err).toMatchObject({
        code: "font-load-check-failed",
        fallbackEligible: false,
      });
    }

    context.measureText = vi.fn(() => ({ width: Number.NaN }));
    expect(() => prepared.measureText("nan", '16px "Inter-z"')).toThrow(
      MmdsBrowserTextMetricsCapabilityError,
    );
    try {
      prepared.measureText("nan-again", '16px "Inter-w"');
    } catch (err) {
      expect(err).toMatchObject({
        code: "font-load-check-failed",
        fallbackEligible: false,
        cssFont: '16px "Inter-w"',
      });
    }
  });

  it("Q6 §3.4 — memoizes measureText results by (text, cssFont)", async () => {
    const { context, environment } = environmentFixture();
    const measureSpy = vi.fn((text: string) => ({ width: text.length * 3 }));
    context.measureText = measureSpy;
    const prepared = await prepareWorkerTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });
    prepared.measureText("hello", "16px Inter");
    prepared.measureText("hello", "16px Inter");
    expect(measureSpy).toHaveBeenCalledTimes(1);
    prepared.measureText("hello", "16px Verdana");
    expect(measureSpy).toHaveBeenCalledTimes(2);
    prepared.measureText("world", "16px Inter");
    expect(measureSpy).toHaveBeenCalledTimes(3);
  });

  it("Q6 §3.2 — multi-style loads and checks each style", async () => {
    const fonts = fontSetFixture();
    const { environment } = environmentFixture(fonts);
    const prepared = await prepareWorkerTextMetrics({
      request: multiStyleRequest(),
      environment,
    });
    expect(fonts.load).toHaveBeenCalledTimes(2);
    expect(fonts.check).toHaveBeenCalledTimes(2);
    const parsed = JSON.parse(prepared.metricsJson);
    expect(
      parsed.textStyles.map((s: { cssFont: string }) => s.cssFont),
    ).toEqual(['normal 400 16px "Inter"', 'normal 400 8px "Verdana"']);
  });

  it("Q6 §3.5 — rejects invalid numeric inputs before measurement", async () => {
    const { environment } = environmentFixture();
    await expect(
      prepareWorkerTextMetrics({
        request: { fontFamily: "Inter", fontSizePx: 0, lineHeightPx: 24 },
        environment,
      }),
    ).rejects.toThrow(/positive/);
    await expect(
      prepareWorkerTextMetrics({
        request: {
          fontFamily: "Inter",
          fontSizePx: 16,
          lineHeightPx: Number.NaN,
        },
        environment,
      }),
    ).rejects.toThrow(/positive/);
    await expect(
      prepareWorkerTextMetrics({
        request: {
          defaultStyle: "s0",
          textStyles: [
            {
              id: "s0",
              fontFamily: "Inter",
              fontSize: 16,
              fontSizePx: 17,
            },
          ],
        },
        environment,
      }),
    ).rejects.toThrow(/match/);
  });

  it("Q6 §3.2 (negative) — one bad style rejects with font-load-check-failed", async () => {
    const fonts = fontSetFixture({
      load: vi.fn(async () => [{}]),
      check: vi.fn((cssFont: string) => !cssFont.includes("Verdana")),
    });
    const { environment } = environmentFixture(fonts);
    await expect(
      prepareWorkerTextMetrics({
        request: multiStyleRequest(),
        environment,
      }),
    ).rejects.toMatchObject({
      code: "font-load-check-failed",
      fallbackEligible: false,
    });
  });
});
