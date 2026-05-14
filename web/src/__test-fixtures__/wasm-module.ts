import { vi } from "vitest";

export interface MockWasmModule {
  default: () => Promise<void>;
  browserTextMetricsRequest: (
    input: string,
    format: string,
    configJson: string,
  ) => string;
  render: (input: string, format: string, configJson: string) => string;
  renderWithBrowserTextMetrics: (
    input: string,
    format: string,
    configJson: string,
    metricsJson: string,
    measureText: (text: string, cssFont: string) => number,
  ) => string;
  validate: (input: string) => string;
}

export function wasmModuleFixture(
  renderWithBrowserTextMetrics = vi.fn(
    (
      input: string,
      format: string,
      configJson: string,
      metricsJson: string,
      callback: (text: string, cssFont: string) => number,
    ) =>
      `${format}:${input}:${configJson}:${metricsJson}:${callback("A", "font")}`,
  ),
) {
  const initialize = vi.fn(async () => {});
  const browserTextMetricsRequest = vi.fn(() => '{"required":false}');
  const render = vi.fn(() => "static unused");
  const validate = vi.fn(() => '{"valid":true}');
  const module: MockWasmModule = {
    default: initialize,
    browserTextMetricsRequest,
    render,
    renderWithBrowserTextMetrics,
    validate,
  };

  return {
    initialize,
    module,
    browserTextMetricsRequest,
    render,
    renderWithBrowserTextMetrics,
    validate,
  };
}
