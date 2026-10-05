import { createWorkerRequestHandler } from "@mmds/browser-text-metrics/worker";
import type {
  WorkerRequestMessage,
  WorkerResponseMessage,
} from "@mmds/browser-text-metrics/worker-protocol";
import {
  createFittedRenderHandler,
  type FittedRenderResultMessage,
  isFittedRenderRequestMessage,
} from "./services/fitted-worker";
import { loadWasmModule } from "./wasm-module";

export { createWorkerRequestHandler };
export type { WorkerRequestMessage, WorkerResponseMessage };

interface WorkerScope {
  postMessage: (
    message: WorkerResponseMessage | FittedRenderResultMessage,
  ) => void;
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
    loadWasmModule,
    postMessage: (message) => {
      workerScope.postMessage(message);
    },
  });

  const fittedHandler = createFittedRenderHandler({
    loadWasmModule,
    postMessage: (message) => {
      workerScope.postMessage(message);
    },
  });

  workerScope.onmessage = (event: MessageEvent<WorkerRequestMessage>) => {
    const message: unknown = event.data;
    void (isFittedRenderRequestMessage(message)
      ? fittedHandler(message)
      : handler(message));
  };
}
