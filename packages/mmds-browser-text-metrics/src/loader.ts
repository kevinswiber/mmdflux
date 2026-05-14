export interface MmdsWasmExports {
  render: (input: string, format: string, configJson: string) => string;
  renderWithBrowserTextMetrics: (
    input: string,
    format: string,
    configJson: string,
    metricsJson: string,
    measureText: (text: string, cssFont: string) => number,
  ) => string;
  browserTextMetricsRequest: (
    input: string,
    format: string,
    configJson: string,
  ) => string;
  validate: (input: string) => string;
  /**
   * Optional. The published `@mmds/wasm` bundler target self-initializes
   * via `__wbindgen_start()` at import time, so callers must NOT assume
   * `default` is present. Only the playground's local `--target web`
   * build exports a default initializer.
   */
  default?: (init?: unknown) => Promise<unknown>;
  detect?: (input: string) => string | undefined;
  version?: () => string;
}

export type MmdsWasmModuleLoader = () => Promise<MmdsWasmExports>;

/**
 * The single static reference to `@mmds/wasm` in this package. Every other
 * module that needs wasm receives a `MmdsWasmModuleLoader` via dependency
 * injection so SSR consumers can import `./index` without evaluating wasm.
 *
 * The dynamic import is wrapped so a bundler/runtime resolution failure
 * surfaces with an actionable message: the most common cause is the host
 * app forgetting to install the `@mmds/wasm` peer dependency.
 */
export const loadMmdsWasm: MmdsWasmModuleLoader = async () => {
  try {
    return (await import("@mmds/wasm")) as unknown as MmdsWasmExports;
  } catch (error) {
    throw new Error(
      "Failed to import the @mmds/wasm peer dependency. Install @mmds/wasm in the host app and ensure your bundler can resolve it.",
      { cause: error },
    );
  }
};

const REQUIRED_METHODS = [
  "render",
  "renderWithBrowserTextMetrics",
  "browserTextMetricsRequest",
  "validate",
] as const;

/**
 * Runtime guard for the published `@mmds/wasm` export shape. Tree-shakable
 * and kept off the hot path — the cross-package integration job
 * calls it once after `loadMmdsWasm()` so generated-d.ts drift surfaces
 * with a clear error instead of a runtime TypeError later.
 */
export function assertMmdsWasmExports(
  value: unknown,
): asserts value is MmdsWasmExports {
  if (typeof value !== "object" || value === null) {
    throw new Error("Expected an object from @mmds/wasm import.");
  }
  const v = value as Record<string, unknown>;
  for (const name of REQUIRED_METHODS) {
    if (typeof v[name] !== "function") {
      throw new Error(`@mmds/wasm export "${name}" must be a function.`);
    }
  }
  if (v.default !== undefined && typeof v.default !== "function") {
    throw new Error(
      '@mmds/wasm export "default" must be a function if present.',
    );
  }
  if (v.detect !== undefined && typeof v.detect !== "function") {
    throw new Error(
      '@mmds/wasm export "detect" must be a function if present.',
    );
  }
  if (v.version !== undefined && typeof v.version !== "function") {
    throw new Error(
      '@mmds/wasm export "version" must be a function if present.',
    );
  }
}
