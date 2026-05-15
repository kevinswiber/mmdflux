import { describe, expect, it, vi } from "vitest";
import { MmdsBrowserTextMetricsCapabilityError } from "../src/capability.js";
import { buildCssFont } from "../src/css-font.js";
import {
  type MainThreadBrowserTextMetricsEnvironment,
  prepareMainThreadTextMetrics,
  prepareWorkerTextMetrics,
} from "../src/prepare.js";
import {
  environmentFixture,
  fontSetFixture,
  mainThreadEnvironmentFixture,
  multiStyleRequest,
} from "./_fixtures.js";

describe("prepareWorkerTextMetrics (migrated playground)", () => {
  it("fails honestly without OffscreenCanvas", async () => {
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

  it("fails honestly without worker FontFaceSet support", async () => {
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

  it("classifies missing 2D canvas context as fallback-eligible", async () => {
    class FakeOffscreenCanvasWithout2dContext {
      getContext(): null {
        return null;
      }
    }

    await expect(
      prepareWorkerTextMetrics({
        request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
        environment: {
          OffscreenCanvas: FakeOffscreenCanvasWithout2dContext,
          fonts: fontSetFixture(),
        },
      }),
    ).rejects.toMatchObject({
      code: "worker-canvas-2d-context-unavailable",
      fallbackEligible: true,
    });
  });

  it("uses load for readiness and post-load check for validity", async () => {
    const calls: string[] = [];
    const fonts = fontSetFixture({
      load: vi.fn(async () => {
        calls.push("load");
        return [{}];
      }),
      ready: new Promise(() => {}),
      check: vi.fn(() => {
        calls.push("check");
        return true;
      }),
    });
    const { environment } = environmentFixture(fonts);

    const prepared = await prepareWorkerTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });

    expect(fonts.load).toHaveBeenCalledWith('normal 400 16px "Inter"');
    expect(fonts.check).toHaveBeenCalledWith('normal 400 16px "Inter"');
    expect(calls).toEqual(["load", "check"]);
    expect(JSON.parse(prepared.metricsJson)).toEqual({
      defaultStyle: "s0",
      profileId: "mmdflux-browser-canvas-v1",
      profileVersion: 1,
      textStyles: [
        {
          id: "s0",
          cssFont: 'normal 400 16px "Inter"',
          fontFamily: "Inter",
          fontSize: 16,
          lineHeight: 24,
          fontStyle: "normal",
          fontWeight: "400",
        },
      ],
    });
  });

  it("accepts system fonts that pass post-load validation without loaded font faces", async () => {
    const { environment } = environmentFixture(
      fontSetFixture({
        load: vi.fn(async () => []),
        check: vi.fn(() => true),
      }),
    );

    await expect(
      prepareWorkerTextMetrics({
        request: { fontFamily: "Arial", fontSizePx: 16, lineHeightPx: 24 },
        environment,
      }),
    ).resolves.toMatchObject({
      metricsJson: expect.stringContaining('"fontFamily":"Arial"'),
    });
  });

  it("does not classify failed post-load checks as fallback-eligible", async () => {
    const { environment } = environmentFixture(
      fontSetFixture({ check: vi.fn(() => false) }),
    );

    await expect(
      prepareWorkerTextMetrics({
        request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
        environment,
      }),
    ).rejects.toThrow("unavailable");
    await expect(
      prepareWorkerTextMetrics({
        request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
        environment,
      }),
    ).rejects.not.toMatchObject({ fallbackEligible: true });
  });

  it("returns a synchronous finite width from canvas measureText", async () => {
    const { context, environment } = environmentFixture();
    const prepared = await prepareWorkerTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });

    expect(prepared.measureText("Alpha", 'normal 400 16px "Inter"')).toBe(15);
    expect(context.font).toBe('normal 400 16px "Inter"');
  });

  it("caches repeated exact text and font measurements", async () => {
    const { context, environment } = environmentFixture();
    const prepared = await prepareWorkerTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });

    prepared.measureText("Alpha", 'normal 400 16px "Inter"');
    prepared.measureText("Alpha", 'normal 400 16px "Inter"');
    prepared.measureText("Alpha", 'normal 400 18px "Inter"');

    expect(context.measureText).toHaveBeenCalledTimes(2);
  });

  it("preflights and caches each css font in a style set", async () => {
    const fonts = fontSetFixture();
    const { context, environment } = environmentFixture(fonts);

    const prepared = await prepareWorkerTextMetrics({
      request: multiStyleRequest(),
      environment,
    });

    expect(fonts.load).toHaveBeenCalledWith('normal 400 16px "Inter"');
    expect(fonts.load).toHaveBeenCalledWith('normal 400 8px "Verdana"');
    expect(fonts.check).toHaveBeenCalledWith('normal 400 16px "Inter"');
    expect(fonts.check).toHaveBeenCalledWith('normal 400 8px "Verdana"');
    expect(JSON.parse(prepared.metricsJson)).toMatchObject({
      defaultStyle: "s0",
      textStyles: [
        {
          id: "s0",
          cssFont: 'normal 400 16px "Inter"',
          fontFamily: "Inter",
          fontSize: 16,
          fontStyle: "normal",
          fontWeight: "400",
          lineHeight: 24,
        },
        {
          id: "s1",
          cssFont: 'normal 400 8px "Verdana"',
          fontFamily: "Verdana",
          fontSize: 8,
          fontStyle: "normal",
          fontWeight: "400",
          lineHeight: 12,
        },
      ],
      profileId: "mmdflux-browser-canvas-v1",
      profileVersion: 1,
    });

    prepared.measureText("Same", 'normal 400 16px "Inter"');
    prepared.measureText("Same", 'normal 400 16px "Inter"');
    prepared.measureText("Same", 'normal 400 8px "Verdana"');

    expect(context.measureText).toHaveBeenCalledTimes(2);
  });

  it("rejects when any style-set font is unavailable", async () => {
    const { environment } = environmentFixture(
      fontSetFixture({
        check: vi.fn((cssFont) => !cssFont.includes("Verdana")),
      }),
    );

    await expect(
      prepareWorkerTextMetrics({
        request: multiStyleRequest(),
        environment,
      }),
    ).rejects.toThrow("Verdana");
  });

  it("quotes CSS font families and rejects invalid numeric style fields", async () => {
    expect(
      buildCssFont({
        fontFamily: "Open Sans",
        fontSizePx: 16,
        lineHeightPx: 24,
      }),
    ).toBe('normal 400 16px "Open Sans"');

    expect(
      buildCssFont({
        fontFamily: 'Arial, "Trebuchet MS", sans-serif',
        fontSizePx: 16,
        lineHeightPx: 24,
      }),
    ).toBe('normal 400 16px "Arial", "Trebuchet MS", sans-serif');

    await expect(
      prepareWorkerTextMetrics({
        request: { fontFamily: "Inter", fontSizePx: 0, lineHeightPx: 24 },
        environment: environmentFixture().environment,
      }),
    ).rejects.toThrow("fontSize");

    await expect(
      prepareWorkerTextMetrics({
        request: {
          fontFamily: "Inter",
          fontSizePx: 16,
          lineHeightPx: Number.NaN,
        },
        environment: environmentFixture().environment,
      }),
    ).rejects.toThrow("lineHeight");

    await expect(
      prepareWorkerTextMetrics({
        request: {
          defaultStyle: "s0",
          textStyles: [
            {
              id: "s0",
              fontFamily: "Inter",
              fontSize: 16,
              fontSizePx: 18,
              lineHeight: 24,
              cssFont: "16px Inter",
            },
          ],
        },
        environment: environmentFixture().environment,
      }),
    ).rejects.toThrow("fontSize");

    await expect(
      prepareWorkerTextMetrics({
        request: {
          defaultStyle: "s0",
          textStyles: [
            {
              id: "s0",
              fontFamily: "Inter",
              fontSize: 16,
              lineHeight: 24,
              lineHeightPx: 30,
              cssFont: "16px Inter",
            },
          ],
        },
        environment: environmentFixture().environment,
      }),
    ).rejects.toThrow("lineHeight");
  });
});

describe("prepareMainThreadTextMetrics (migrated playground)", () => {
  it("prepares main-thread metrics with document fonts and a canvas", async () => {
    const calls: string[] = [];
    const fonts = fontSetFixture({
      load: vi.fn(async () => {
        calls.push("load");
        return [{}];
      }),
      ready: Promise.resolve(),
      check: vi.fn(() => {
        calls.push("check");
        return true;
      }),
    });
    const { context, environment } = mainThreadEnvironmentFixture(fonts);

    const prepared = await prepareMainThreadTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });

    expect(calls).toEqual(["load", "check"]);
    expect(fonts.load).toHaveBeenCalledWith('normal 400 16px "Inter"');
    expect(fonts.check).toHaveBeenCalledWith('normal 400 16px "Inter"');
    expect(JSON.parse(prepared.metricsJson)).toMatchObject({
      profileId: "mmdflux-browser-canvas-v1",
      profileVersion: 1,
      textStyles: [
        expect.objectContaining({
          fontStyle: "normal",
          fontWeight: "400",
        }),
      ],
    });
    expect(prepared.measureText("Alpha", 'normal 400 16px "Inter"')).toBe(15);
    expect(context.font).toBe('normal 400 16px "Inter"');
  });

  it("fails clearly without main-thread FontFaceSet support", async () => {
    const { environment } = mainThreadEnvironmentFixture();
    if (!environment.document) {
      throw new Error("fixture did not create a document");
    }
    environment.document.fonts = undefined;

    await expect(
      prepareMainThreadTextMetrics({
        request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
        environment,
      }),
    ).rejects.toMatchObject({
      code: "main-thread-font-face-set-unavailable",
      fallbackEligible: false,
    });
  });

  it("fails clearly without a main-thread canvas context", async () => {
    const environment: MainThreadBrowserTextMetricsEnvironment = {
      document: {
        fonts: fontSetFixture(),
        createElement: vi.fn(() => ({
          getContext: () => null,
        })),
      },
    };

    await expect(
      prepareMainThreadTextMetrics({
        request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
        environment,
      }),
    ).rejects.toMatchObject({
      code: "main-thread-canvas-2d-context-unavailable",
      fallbackEligible: false,
    });
  });

  it("does not classify main-thread unavailable fonts as fallback-eligible", async () => {
    const { environment } = mainThreadEnvironmentFixture(
      fontSetFixture({ check: vi.fn(() => false) }),
    );

    await expect(
      prepareMainThreadTextMetrics({
        request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
        environment,
      }),
    ).rejects.toThrow("unavailable");
    await expect(
      prepareMainThreadTextMetrics({
        request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
        environment,
      }),
    ).rejects.not.toMatchObject({ fallbackEligible: true });
  });

  it("caches repeated main-thread measurements per prepared provider", async () => {
    const { context, environment } = mainThreadEnvironmentFixture();
    const first = await prepareMainThreadTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });
    const second = await prepareMainThreadTextMetrics({
      request: { fontFamily: "Inter", fontSizePx: 16, lineHeightPx: 24 },
      environment,
    });

    first.measureText("Alpha", 'normal 400 16px "Inter"');
    first.measureText("Alpha", 'normal 400 16px "Inter"');
    second.measureText("Alpha", 'normal 400 16px "Inter"');

    expect(context.measureText).toHaveBeenCalledTimes(2);
  });
});
