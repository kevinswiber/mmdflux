// Ambient declaration so the package compiles without installing the
// `@mmds/wasm` peer dependency. The real type ships with the published
// peer (bundler target — see crates/mmdflux-wasm). The cross-package
// integration job in packages-ci.yml verifies the runtime shape against
// the packed tarball.
declare module "@mmds/wasm" {
  export const render: (
    input: string,
    format: string,
    configJson: string,
  ) => string;
  export const renderWithBrowserTextMetrics: (
    input: string,
    format: string,
    configJson: string,
    metricsJson: string,
    measureText: (text: string, cssFont: string) => number,
  ) => string;
  export const browserTextMetricsRequest: (
    input: string,
    format: string,
    configJson: string,
  ) => string;
  export const validate: (input: string) => string;
  export const detect: ((input: string) => string | undefined) | undefined;
  export const version: (() => string) | undefined;
  // Only the web target emits a default initializer; the bundler target
  // self-initializes via __wbindgen_start at import time.
  const initialize: ((init?: unknown) => Promise<unknown>) | undefined;
  export default initialize;
}
