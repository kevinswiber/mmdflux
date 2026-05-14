import { describe, expect, it, vi } from "vitest";
import { MmdsBrowserTextMetricsCapabilityError } from "../src/capability.js";
import type { MmdsWasmExports } from "../src/loader.js";
import { createMmdsMainThreadRenderer } from "../src/main-thread.js";

function wasmModuleFixture(
  overrides: Partial<MmdsWasmExports> = {},
): MmdsWasmExports {
  return {
    default: vi.fn(async () => undefined),
    render: vi.fn(
      (input: string, format: string, configJson: string) =>
        `static:${format}:${input}:${configJson}`,
    ),
    renderWithBrowserTextMetrics: vi.fn(
      (
        input: string,
        format: string,
        configJson: string,
        metricsJson: string,
        measureText: (text: string, cssFont: string) => number,
      ) =>
        `dynamic:${format}:${input}:${configJson}:${metricsJson}:${measureText("A", "f")}`,
    ),
    browserTextMetricsRequest: vi.fn(() => '{"required":false}'),
    validate: vi.fn(() => '{"valid":true}'),
    ...overrides,
  };
}

describe("createMmdsMainThreadRenderer", () => {
  it("does not import or initialize wasm at construction time", () => {
    const fixture = wasmModuleFixture();
    const loadWasmModule = vi.fn(async () => fixture);
    createMmdsMainThreadRenderer({ loadWasmModule });
    expect(loadWasmModule).not.toHaveBeenCalled();
    expect(fixture.default).not.toHaveBeenCalled();
  });

  it("renderSvg prepares metrics and calls wasm dynamic export with source: main-thread", async () => {
    const measureText = vi.fn(() => 42);
    const prepared = {
      metricsJson: '{"cssFont":"16px Inter"}',
      measureText,
    };
    const prepareMainThreadTextMetrics = vi.fn(async () => prepared);
    const fixture = wasmModuleFixture();
    const loadWasmModule = vi.fn(async () => fixture);
    const renderer = createMmdsMainThreadRenderer({
      loadWasmModule,
      prepareMainThreadTextMetrics,
    });

    const result = await renderer.renderSvg({
      input: "graph TD\nA-->B",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    });

    expect(prepareMainThreadTextMetrics).toHaveBeenCalledWith({
      request: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
      environment: undefined,
    });
    expect(fixture.renderWithBrowserTextMetrics).toHaveBeenCalledWith(
      "graph TD\nA-->B",
      "svg",
      "{}",
      '{"cssFont":"16px Inter"}',
      measureText,
    );
    expect(result).toEqual({
      output: 'dynamic:svg:graph TD\nA-->B:{}:{"cssFont":"16px Inter"}:42',
      format: "svg",
      source: "main-thread",
    });
  });

  it("initializes wasm once across calls; prepares fresh metrics each time", async () => {
    const firstMeasure = vi.fn(() => 1);
    const secondMeasure = vi.fn(() => 2);
    const prepareMainThreadTextMetrics = vi
      .fn()
      .mockResolvedValueOnce({
        metricsJson: '{"cssFont":"first"}',
        measureText: firstMeasure,
      })
      .mockResolvedValueOnce({
        metricsJson: '{"cssFont":"second"}',
        measureText: secondMeasure,
      });
    const fixture = wasmModuleFixture();
    const loadWasmModule = vi.fn(async () => fixture);
    const renderer = createMmdsMainThreadRenderer({
      loadWasmModule,
      prepareMainThreadTextMetrics,
    });

    await renderer.renderSvg({
      input: "g1",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    });
    await renderer.renderSvg({
      input: "g2",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    });

    expect(loadWasmModule).toHaveBeenCalledTimes(1);
    expect(fixture.default).toHaveBeenCalledTimes(1);
    expect(prepareMainThreadTextMetrics).toHaveBeenCalledTimes(2);
    expect(firstMeasure).toHaveBeenCalledTimes(1);
    expect(secondMeasure).toHaveBeenCalledTimes(1);
  });

  it("rejects preparation failures without loading wasm", async () => {
    const fixture = wasmModuleFixture();
    const loadWasmModule = vi.fn(async () => fixture);
    const renderer = createMmdsMainThreadRenderer({
      loadWasmModule,
      prepareMainThreadTextMetrics: vi.fn(async () => {
        throw new MmdsBrowserTextMetricsCapabilityError({
          code: "main-thread-font-face-set-unavailable",
          message: "no fonts",
        });
      }),
    });

    await expect(
      renderer.renderSvg({
        input: "g",
        browserTextMetrics: {
          fontFamily: "Inter",
          fontSizePx: 16,
          lineHeightPx: 24,
        },
      }),
    ).rejects.toMatchObject({
      code: "main-thread-font-face-set-unavailable",
    });
    expect(loadWasmModule).not.toHaveBeenCalled();
  });

  it("renderStatic routes svg/text/ascii/mmds/mermaid through wasm.render with source: static", async () => {
    const fixture = wasmModuleFixture();
    const loadWasmModule = vi.fn(async () => fixture);
    const renderer = createMmdsMainThreadRenderer({ loadWasmModule });

    const formats = ["svg", "text", "ascii", "mmds", "mermaid"] as const;
    for (const format of formats) {
      const result = await renderer.renderStatic({
        input: "graph TD\nA-->B",
        format,
      });
      expect(result).toEqual({
        output: `static:${format}:graph TD\nA-->B:{}`,
        format,
        source: "static",
      });
    }
    expect(fixture.render).toHaveBeenCalledTimes(formats.length);
    expect(fixture.renderWithBrowserTextMetrics).not.toHaveBeenCalled();
  });

  it("validate calls wasm.validate and returns the result JSON", async () => {
    const fixture = wasmModuleFixture({
      validate: vi.fn((input: string) => `{"valid":true,"input":"${input}"}`),
    });
    const loadWasmModule = vi.fn(async () => fixture);
    const renderer = createMmdsMainThreadRenderer({ loadWasmModule });

    const result = await renderer.validate("graph TD\nA-->B");
    expect(fixture.validate).toHaveBeenCalledWith("graph TD\nA-->B");
    expect(result).toBe('{"valid":true,"input":"graph TD\nA-->B"}');
  });

  it("renderSvg cannot accept a format field (TS compile-time guard)", async () => {
    const renderer = createMmdsMainThreadRenderer({
      loadWasmModule: async () => wasmModuleFixture(),
      prepareMainThreadTextMetrics: async () => ({
        metricsJson: "{}",
        measureText: () => 0,
      }),
    });
    await renderer
      // @ts-expect-error renderSvg always renders svg; format is not part of the input
      .renderSvg({
        input: "g",
        format: "text",
        browserTextMetrics: {
          fontFamily: "Inter",
          fontSizePx: 16,
          lineHeightPx: 24,
        },
      })
      .catch(() => {
        // tolerate any runtime fallout; this case is purely a compile-time check
      });
  });

  it("classifies wasm render rejections with classifyWasmError", async () => {
    const fixture = wasmModuleFixture({
      render: vi.fn(() => {
        throw new Error("renderWithBrowserTextMetrics re-entered");
      }),
    });
    const loadWasmModule = vi.fn(async () => fixture);
    const renderer = createMmdsMainThreadRenderer({ loadWasmModule });

    await expect(
      renderer.renderStatic({ input: "g", format: "svg" }),
    ).rejects.toMatchObject({
      code: "wasm-reentered",
      fallbackEligible: false,
    });
  });

  it("does not require default() on the wasm module (bundler-target self-init)", async () => {
    const fixture = wasmModuleFixture();
    (fixture as { default?: unknown }).default = undefined;
    const loadWasmModule = vi.fn(async () => fixture);
    const renderer = createMmdsMainThreadRenderer({ loadWasmModule });
    const result = await renderer.renderStatic({
      input: "g",
      format: "svg",
    });
    expect(result.source).toBe("static");
    expect(loadWasmModule).toHaveBeenCalledTimes(1);
  });
});
