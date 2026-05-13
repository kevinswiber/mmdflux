export interface WasmModule {
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

export async function loadWasmModule(): Promise<WasmModule> {
  return (await import("./wasm-pkg/mmdflux_wasm.js")) as unknown as WasmModule;
}
