import { isMmdsBrowserTextMetricsCapabilityError } from "./capability.js";
import type { MmdsWasmExports, MmdsWasmModuleLoader } from "./loader.js";
import {
  type BrowserTextMetricsEnvironment,
  prepareWorkerTextMetrics as defaultPrepareWorkerTextMetrics,
  type PreparedBrowserTextMetrics,
  type PrepareWorkerTextMetricsOptions,
} from "./prepare.js";
import { runWasm } from "./wasm-classifier.js";
import {
  isWorkerBrowserTextMetricsDecision,
  isWorkerRequestMessage,
  PROTOCOL_VERSION,
  type WorkerErrorMessage,
  type WorkerResponseMessage,
} from "./worker-protocol.js";

export type { MmdsWasmExports, MmdsWasmModuleLoader } from "./loader.js";
// Re-export so existing callers that imported from `./worker` keep
// compiling; new code should import from `./wasm-classifier` directly.
export { classifyWasmError } from "./wasm-classifier.js";

export type PrepareWorkerTextMetricsFn = (
  options: PrepareWorkerTextMetricsOptions,
) => Promise<PreparedBrowserTextMetrics>;

export interface WorkerRequestHandlerOptions {
  readonly loadWasmModule: MmdsWasmModuleLoader;
  readonly postMessage: (message: WorkerResponseMessage) => void;
  readonly prepareBrowserTextMetrics?: PrepareWorkerTextMetricsFn;
  readonly environment?: BrowserTextMetricsEnvironment;
}

export function createWorkerRequestHandler(
  options: WorkerRequestHandlerOptions,
): (message: unknown) => Promise<void> {
  const loadModule = options.loadWasmModule;
  const prepareMetrics =
    options.prepareBrowserTextMetrics ?? defaultPrepareWorkerTextMetrics;
  const postMessage = options.postMessage;
  let modulePromise: Promise<MmdsWasmExports> | null = null;

  const getWasmModule = async (): Promise<MmdsWasmExports> => {
    if (!modulePromise) {
      modulePromise = loadModule().then(async (module) => {
        if (module.default) {
          await module.default();
        }
        return module;
      });
    }
    return modulePromise;
  };

  return async (message: unknown): Promise<void> => {
    const seq = salvageSeq(message);

    if (
      typeof message !== "object" ||
      message === null ||
      (message as { version?: unknown }).version !== PROTOCOL_VERSION
    ) {
      const received = (message as { version?: unknown } | null)?.version;
      postMessage({
        version: PROTOCOL_VERSION,
        type: "error",
        seq,
        error: `Unsupported worker-protocol version: ${String(received)}`,
        code: "unsupported-format",
      });
      return;
    }

    if (!isWorkerRequestMessage(message)) {
      postMessage({
        version: PROTOCOL_VERSION,
        type: "error",
        seq,
        error: "Malformed worker request payload.",
        code: "unsupported-format",
      });
      return;
    }

    try {
      const wasmModule = await getWasmModule();
      if (message.type === "render") {
        const output = runWasm(() =>
          wasmModule.render(message.input, message.format, message.configJson),
        );
        postMessage({
          version: PROTOCOL_VERSION,
          type: "result",
          seq: message.seq,
          format: message.format,
          output,
        });
        return;
      }

      if (message.type === "resolveBrowserTextMetrics") {
        const decisionJson = runWasm(() =>
          wasmModule.browserTextMetricsRequest(
            message.input,
            message.format,
            message.configJson,
          ),
        );
        const parsed: unknown = JSON.parse(decisionJson);
        if (!isWorkerBrowserTextMetricsDecision(parsed)) {
          throw new Error(
            "browserTextMetricsRequest returned a malformed decision payload.",
          );
        }
        postMessage({
          version: PROTOCOL_VERSION,
          type: "browserTextMetricsDecision",
          seq: message.seq,
          decision: parsed,
        });
        return;
      }

      if (message.type === "renderWithBrowserTextMetrics") {
        const prepared = await prepareMetrics({
          request: message.browserTextMetrics,
          environment: options.environment,
        });
        const output = runWasm(() =>
          wasmModule.renderWithBrowserTextMetrics(
            message.input,
            message.format,
            message.configJson,
            prepared.metricsJson,
            prepared.measureText,
          ),
        );
        postMessage({
          version: PROTOCOL_VERSION,
          type: "result",
          seq: message.seq,
          format: message.format,
          output,
        });
        return;
      }

      const resultJson = runWasm(() => wasmModule.validate(message.input));
      postMessage({
        version: PROTOCOL_VERSION,
        type: "validation",
        seq: message.seq,
        resultJson,
      });
    } catch (error) {
      postMessage(errorResponse(message.seq, error));
    }
  };
}

function errorResponse(seq: number, error: unknown): WorkerErrorMessage {
  const formatted = formatError(error);
  if (isMmdsBrowserTextMetricsCapabilityError(error)) {
    if (
      error.code === "wasm-render-rejected" ||
      error.code === "wasm-config-rejected" ||
      error.code === "wasm-reentered"
    ) {
      return {
        version: PROTOCOL_VERSION,
        type: "error",
        seq,
        error: formatted,
        code: error.code,
      };
    }
    if (error.fallbackEligible) {
      return {
        version: PROTOCOL_VERSION,
        type: "error",
        seq,
        error: formatted,
        code: "dynamic-metrics-capability",
      };
    }
  }
  return {
    version: PROTOCOL_VERSION,
    type: "error",
    seq,
    error: formatted,
  };
}

function salvageSeq(message: unknown): number {
  const candidate = (message as { seq?: unknown } | null)?.seq;
  return typeof candidate === "number" ? candidate : 0;
}

function formatError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}
