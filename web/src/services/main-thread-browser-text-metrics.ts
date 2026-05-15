import { createMmdsMainThreadRenderer } from "@mmds/browser-text-metrics/main-thread";
import type { prepareMainThreadBrowserTextMetrics as defaultPrepareMainThreadBrowserTextMetrics } from "../browser-text-metrics";
import { loadWasmModule, type WasmModule } from "../wasm-module";
import type {
  BrowserTextMetricsRenderRequest,
  RenderResponse,
} from "./render-client";

export interface MainThreadBrowserTextMetricsRenderer {
  renderWithBrowserTextMetrics: (
    request: BrowserTextMetricsRenderRequest,
  ) => Promise<RenderResponse>;
}

export interface MainThreadBrowserTextMetricsRendererOptions {
  loadWasmModule?: () => Promise<WasmModule>;
  prepareMainThreadBrowserTextMetrics?: typeof defaultPrepareMainThreadBrowserTextMetrics;
}

export function createMainThreadBrowserTextMetricsRenderer(
  options: MainThreadBrowserTextMetricsRendererOptions = {},
): MainThreadBrowserTextMetricsRenderer {
  const prepareLegacy = options.prepareMainThreadBrowserTextMetrics;
  const renderer = createMmdsMainThreadRenderer({
    loadWasmModule: options.loadWasmModule ?? loadWasmModule,
    prepareMainThreadTextMetrics: prepareLegacy
      ? ({ request }) => prepareLegacy(request)
      : undefined,
  });

  return {
    renderWithBrowserTextMetrics: async (request) => {
      const result = await renderer.renderSvg({
        input: request.input,
        browserTextMetrics: request.browserTextMetrics,
        configJson: request.configJson ?? "{}",
      });
      return {
        seq: request.seq,
        format: "svg",
        output: result.output,
      };
    },
  };
}
