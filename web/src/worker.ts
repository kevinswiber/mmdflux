import type {
  WorkerRequestMessage,
  WorkerResponseMessage,
} from "./worker-protocol";

export type {
  WorkerRequestMessage,
  WorkerResponseMessage,
} from "./worker-protocol";

interface WasmModule {
  default: () => Promise<void>;
  render: (input: string, format: string, configJson: string) => string;
  validate: (input: string) => string;
}

interface RenderRequestHandlerOptions {
  loadWasmModule?: () => Promise<WasmModule>;
  postMessage: (message: WorkerResponseMessage) => void;
}

export async function loadWasmModule(): Promise<WasmModule> {
  return (await import("./wasm-pkg/mmdflux_wasm.js")) as unknown as WasmModule;
}

export function createWorkerRequestHandler(
  options: RenderRequestHandlerOptions,
): (message: WorkerRequestMessage) => Promise<void> {
  const loadModule = options.loadWasmModule ?? loadWasmModule;
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
          type: "result",
          seq: message.seq,
          format: message.format,
          output,
        });
        return;
      }

      const resultJson = wasmModule.validate(message.input);
      postMessage({
        type: "validation",
        seq: message.seq,
        resultJson,
      });
    } catch (error) {
      postMessage({
        type: "error",
        seq: message.seq,
        error: formatError(error),
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
