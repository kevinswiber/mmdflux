import {
  isBrowserTextMetricsCapabilityError,
  prepareBrowserTextMetrics,
} from "./browser-text-metrics";
import { loadWasmModule, type WasmModule } from "./wasm-module";
import {
  PROTOCOL_VERSION,
  type WorkerBrowserTextMetricsDecision,
  type WorkerRequestMessage,
  type WorkerResponseMessage,
} from "./worker-protocol";

export type {
  WorkerRequestMessage,
  WorkerResponseMessage,
} from "./worker-protocol";

interface RenderRequestHandlerOptions {
  loadWasmModule?: () => Promise<WasmModule>;
  prepareBrowserTextMetrics?: typeof prepareBrowserTextMetrics;
  postMessage: (message: WorkerResponseMessage) => void;
}

export function createWorkerRequestHandler(
  options: RenderRequestHandlerOptions,
): (message: WorkerRequestMessage) => Promise<void> {
  const loadModule = options.loadWasmModule ?? loadWasmModule;
  const prepareMetrics =
    options.prepareBrowserTextMetrics ?? prepareBrowserTextMetrics;
  const postMessage = options.postMessage;
  let modulePromise: Promise<WasmModule> | null = null;

  const getWasmModule = async (): Promise<WasmModule> => {
    if (!modulePromise) {
      modulePromise = loadModule().then(async (module) => {
        await module.default();
        return module;
      });
    }

    return modulePromise;
  };

  return async (message: WorkerRequestMessage): Promise<void> => {
    try {
      const wasmModule = await getWasmModule();
      if (message.type === "render") {
        const output = wasmModule.render(
          message.input,
          message.format,
          message.configJson,
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
        const decisionJson = wasmModule.browserTextMetricsRequest(
          message.input,
          message.format,
          message.configJson,
        );
        const decision = JSON.parse(
          decisionJson,
        ) as WorkerBrowserTextMetricsDecision;

        postMessage({
          version: PROTOCOL_VERSION,
          type: "browserTextMetricsDecision",
          seq: message.seq,
          decision,
        });
        return;
      }

      if (message.type === "renderWithBrowserTextMetrics") {
        const prepared = await prepareMetrics(message.browserTextMetrics);
        const output = wasmModule.renderWithBrowserTextMetrics(
          message.input,
          message.format,
          message.configJson,
          prepared.metricsJson,
          prepared.measureText,
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

      const resultJson = wasmModule.validate(message.input);
      postMessage({
        version: PROTOCOL_VERSION,
        type: "validation",
        seq: message.seq,
        resultJson,
      });
    } catch (error) {
      postMessage({
        version: PROTOCOL_VERSION,
        type: "error",
        seq: message.seq,
        error: formatError(error),
        ...(isBrowserTextMetricsCapabilityError(error) && error.fallbackEligible
          ? { code: "dynamic-metrics-capability" as const }
          : {}),
      });
    }
  };
}

function formatError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  return String(error);
}

interface WorkerScope {
  postMessage: (message: WorkerResponseMessage) => void;
  onmessage: ((event: MessageEvent<WorkerRequestMessage>) => void) | null;
}

function getWorkerScope(scope: unknown): WorkerScope | null {
  if (typeof window !== "undefined") {
    return null;
  }

  if (typeof scope !== "object" || scope === null) {
    return null;
  }

  const candidate = scope as Partial<WorkerScope>;
  if (typeof candidate.postMessage !== "function") {
    return null;
  }

  if (!("onmessage" in candidate)) {
    return null;
  }

  return candidate as WorkerScope;
}

const workerScope = getWorkerScope(globalThis);
if (workerScope) {
  const handler = createWorkerRequestHandler({
    postMessage: (message) => {
      workerScope.postMessage(message);
    },
  });

  workerScope.onmessage = (event: MessageEvent<WorkerRequestMessage>) => {
    void handler(event.data);
  };
}
