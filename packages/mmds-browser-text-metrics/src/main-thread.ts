import { loadMmdsWasm, type MmdsWasmModuleLoader } from "./loader.js";
import {
  type BrowserTextMetricsRequest,
  prepareMainThreadTextMetrics as defaultPrepareMainThreadTextMetrics,
  type MainThreadBrowserTextMetricsEnvironment,
  type PreparedBrowserTextMetrics,
  type PrepareMainThreadTextMetricsOptions,
} from "./prepare.js";
import { classifyWasmError } from "./wasm-classifier.js";

export type MmdsRenderFormat = "svg" | "text" | "ascii" | "mmds" | "mermaid";

export type MmdsRenderSource = "worker" | "main-thread" | "static";

export interface MmdsRenderResult {
  output: string;
  format: MmdsRenderFormat;
  source: MmdsRenderSource;
}

export interface MmdsDynamicRenderOptions {
  input: string;
  browserTextMetrics: BrowserTextMetricsRequest;
  configJson?: string;
}

export interface MmdsStaticRenderOptions {
  input: string;
  format: MmdsRenderFormat;
  configJson?: string;
}

export type PrepareMainThreadTextMetricsFn = (
  options: PrepareMainThreadTextMetricsOptions,
) => Promise<PreparedBrowserTextMetrics>;

export interface MmdsMainThreadRendererOptions {
  readonly loadWasmModule?: MmdsWasmModuleLoader;
  readonly prepareMainThreadTextMetrics?: PrepareMainThreadTextMetricsFn;
  readonly environment?: MainThreadBrowserTextMetricsEnvironment;
}

export interface MmdsMainThreadRenderer {
  renderSvg(options: MmdsDynamicRenderOptions): Promise<MmdsRenderResult>;
  renderStatic(options: MmdsStaticRenderOptions): Promise<MmdsRenderResult>;
  validate(input: string): Promise<string>;
}

export function createMmdsMainThreadRenderer(
  options: MmdsMainThreadRendererOptions = {},
): MmdsMainThreadRenderer {
  const loadModule = options.loadWasmModule ?? loadMmdsWasm;
  const prepareMetrics =
    options.prepareMainThreadTextMetrics ?? defaultPrepareMainThreadTextMetrics;
  const getWasmModule = createLazyWasmModule(loadModule);

  return {
    async renderSvg(opts) {
      const prepared = await prepareMetrics({
        request: opts.browserTextMetrics,
        environment: options.environment,
      });
      const wasm = await getWasmModule();
      const output = runWasm(() =>
        wasm.renderWithBrowserTextMetrics(
          opts.input,
          "svg",
          opts.configJson ?? "{}",
          prepared.metricsJson,
          prepared.measureText,
        ),
      );
      return { output, format: "svg", source: "main-thread" };
    },
    async renderStatic(opts) {
      const wasm = await getWasmModule();
      const output = runWasm(() =>
        wasm.render(opts.input, opts.format, opts.configJson ?? "{}"),
      );
      return { output, format: opts.format, source: "static" };
    },
    async validate(input) {
      const wasm = await getWasmModule();
      return runWasm(() => wasm.validate(input));
    },
  };
}

/**
 * Shared with the client orchestrator: single-flight wasm
 * initialization that resolves to the loaded module after invoking
 * the optional `default()` initializer when present.
 */
export function createLazyWasmModule(loader: MmdsWasmModuleLoader) {
  let modulePromise: ReturnType<MmdsWasmModuleLoader> | null = null;
  return () => {
    if (!modulePromise) {
      modulePromise = loader().then(async (module) => {
        if (module.default) {
          await module.default();
        }
        return module;
      });
    }
    return modulePromise;
  };
}

function runWasm<T>(call: () => T): T {
  try {
    return call();
  } catch (error) {
    throw classifyWasmError(error);
  }
}
