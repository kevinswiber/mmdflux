import { describe, expect, it, vi } from "vitest";
import { wasmModuleFixture } from "./__test-fixtures__/wasm-module";
import { createMainThreadBrowserTextMetricsRenderer } from "./services/main-thread-browser-text-metrics";
import type { BrowserTextMetricsRenderRequest } from "./services/render-client";

function renderRequest(
  overrides: Partial<BrowserTextMetricsRenderRequest> = {},
): BrowserTextMetricsRenderRequest {
  return {
    seq: 13,
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

describe("createMainThreadBrowserTextMetricsRenderer", () => {
  it("does not import or initialize wasm at service construction time", () => {
    const loadWasmModule = vi.fn(async () => wasmModuleFixture().module);

    createMainThreadBrowserTextMetricsRenderer({
      loadWasmModule,
      prepareMainThreadBrowserTextMetrics: vi.fn(),
    });

    expect(loadWasmModule).not.toHaveBeenCalled();
  });

  it("prepares main-thread metrics and calls the dynamic wasm export", async () => {
    const measureText = vi.fn(() => 42);
    const prepareMainThreadBrowserTextMetrics = vi.fn(async () => ({
      metricsJson: '{"cssFont":"16px Inter"}',
      measureText,
    }));
    const fixture = wasmModuleFixture();
    const loadWasmModule = vi.fn(async () => fixture.module);
    const renderer = createMainThreadBrowserTextMetricsRenderer({
      loadWasmModule,
      prepareMainThreadBrowserTextMetrics,
    });
    const request = renderRequest();

    await expect(
      renderer.renderWithBrowserTextMetrics(request),
    ).resolves.toEqual({
      seq: request.seq,
      format: "svg",
      output: 'svg:graph TD\nA-->B:{}:{"cssFont":"16px Inter"}:42',
    });

    expect(prepareMainThreadBrowserTextMetrics).toHaveBeenCalledWith(
      request.browserTextMetrics,
    );
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
    const prepareMainThreadBrowserTextMetrics = vi
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
    const renderer = createMainThreadBrowserTextMetricsRenderer({
      loadWasmModule,
      prepareMainThreadBrowserTextMetrics,
    });

    await renderer.renderWithBrowserTextMetrics(renderRequest({ seq: 1 }));
    await renderer.renderWithBrowserTextMetrics(renderRequest({ seq: 2 }));

    expect(loadWasmModule).toHaveBeenCalledTimes(1);
    expect(fixture.initialize).toHaveBeenCalledTimes(1);
    expect(prepareMainThreadBrowserTextMetrics).toHaveBeenCalledTimes(2);
    expect(firstMeasure).toHaveBeenCalledTimes(1);
    expect(secondMeasure).toHaveBeenCalledTimes(1);
  });

  it("rejects preparation failures without loading wasm", async () => {
    const loadWasmModule = vi.fn(async () => wasmModuleFixture().module);
    const renderer = createMainThreadBrowserTextMetricsRenderer({
      loadWasmModule,
      prepareMainThreadBrowserTextMetrics: vi.fn(async () => {
        throw new Error("Dynamic text metrics require document.fonts");
      }),
    });

    await expect(
      renderer.renderWithBrowserTextMetrics(renderRequest()),
    ).rejects.toThrow("document.fonts");
    expect(loadWasmModule).not.toHaveBeenCalled();
  });
});
