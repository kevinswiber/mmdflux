import { describe, expect, it, vi } from "vitest";
import { createMmdsMainThreadRenderer } from "../src/main-thread.js";
import { wasmModuleFixture } from "./_fixtures.js";

interface RequestShape {
  input: string;
  configJson?: string;
  browserTextMetrics: {
    fontFamily: string;
    fontSizePx: number;
    lineHeightPx: number;
  };
}

function dynamicRequest(overrides: Partial<RequestShape> = {}): RequestShape {
  return {
    input: "graph TD\nA-->B",
    configJson: "{}",
    browserTextMetrics: {
      fontFamily: "Inter",
      fontSizePx: 16,
      lineHeightPx: 24,
    },
    ...overrides,
  };
}

describe("createMmdsMainThreadRenderer (migrated playground)", () => {
  it("does not import or initialize wasm at construction time", () => {
    const loadWasmModule = vi.fn(async () => wasmModuleFixture().module);

    createMmdsMainThreadRenderer({
      loadWasmModule,
      prepareMainThreadTextMetrics: vi.fn(),
    });

    expect(loadWasmModule).not.toHaveBeenCalled();
  });

  it("prepares main-thread metrics and calls the dynamic wasm export", async () => {
    const measureText = vi.fn(() => 42);
    const prepareMainThreadTextMetrics = vi.fn(async () => ({
      metricsJson: '{"cssFont":"16px Inter"}',
      measureText,
    }));
    const fixture = wasmModuleFixture();
    const loadWasmModule = vi.fn(async () => fixture.module);
    const renderer = createMmdsMainThreadRenderer({
      loadWasmModule,
      prepareMainThreadTextMetrics,
    });
    const request = dynamicRequest();

    await expect(
      renderer.renderSvg({
        input: request.input,
        browserTextMetrics: request.browserTextMetrics,
        configJson: request.configJson,
      }),
    ).resolves.toEqual({
      output: 'svg:graph TD\nA-->B:{}:{"cssFont":"16px Inter"}:42',
      format: "svg",
      source: "main-thread",
    });

    expect(prepareMainThreadTextMetrics).toHaveBeenCalledWith({
      request: request.browserTextMetrics,
      environment: undefined,
    });
    expect(fixture.renderWithBrowserTextMetrics).toHaveBeenCalledWith(
      request.input,
      "svg",
      request.configJson,
      '{"cssFont":"16px Inter"}',
      measureText,
    );
    expect(fixture.render).not.toHaveBeenCalled();
  });

  it("initializes wasm once and prepares fresh metrics for each render", async () => {
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
    const loadWasmModule = vi.fn(async () => fixture.module);
    const renderer = createMmdsMainThreadRenderer({
      loadWasmModule,
      prepareMainThreadTextMetrics,
    });

    await renderer.renderSvg({
      input: dynamicRequest().input,
      browserTextMetrics: dynamicRequest().browserTextMetrics,
    });
    await renderer.renderSvg({
      input: dynamicRequest().input,
      browserTextMetrics: dynamicRequest().browserTextMetrics,
    });

    expect(loadWasmModule).toHaveBeenCalledTimes(1);
    expect(fixture.initialize).toHaveBeenCalledTimes(1);
    expect(prepareMainThreadTextMetrics).toHaveBeenCalledTimes(2);
    expect(firstMeasure).toHaveBeenCalledTimes(1);
    expect(secondMeasure).toHaveBeenCalledTimes(1);
  });

  it("rejects preparation failures without loading wasm", async () => {
    const loadWasmModule = vi.fn(async () => wasmModuleFixture().module);
    const renderer = createMmdsMainThreadRenderer({
      loadWasmModule,
      prepareMainThreadTextMetrics: vi.fn(async () => {
        throw new Error("Dynamic text metrics require document.fonts");
      }),
    });

    await expect(
      renderer.renderSvg({
        input: dynamicRequest().input,
        browserTextMetrics: dynamicRequest().browserTextMetrics,
      }),
    ).rejects.toThrow("document.fonts");
    expect(loadWasmModule).not.toHaveBeenCalled();
  });
});
