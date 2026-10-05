import {
  PROTOCOL_VERSION,
  type WorkerErrorMessage,
  type WorkerOutputFormat,
} from "@mmds/browser-text-metrics/worker-protocol";
import type { FitReport } from "../fit-report";

// Fitted renders are a playground-local extension of the shared worker
// protocol: the worker answers these messages itself and hands everything
// else to the `@mmds/browser-text-metrics` handler.
export const FITTED_RENDER_REQUEST = "renderFitted";
export const FITTED_RENDER_RESULT = "fittedResult";

export interface FittedRenderRequestMessage {
  version: typeof PROTOCOL_VERSION;
  type: typeof FITTED_RENDER_REQUEST;
  seq: number;
  input: string;
  format: WorkerOutputFormat;
  configJson: string;
  fitJson: string;
}

export interface FittedRenderResultMessage {
  version: typeof PROTOCOL_VERSION;
  type: typeof FITTED_RENDER_RESULT;
  seq: number;
  format: WorkerOutputFormat;
  output: string;
  fit: FitReport;
}

export interface FittedRenderWasmModule {
  default?: () => Promise<unknown>;
  renderFitted: (
    input: string,
    format: string,
    configJson: string,
    fitJson: string,
  ) => string;
}

interface FittedRenderHandlerOptions {
  loadWasmModule: () => Promise<FittedRenderWasmModule>;
  postMessage: (
    message: FittedRenderResultMessage | WorkerErrorMessage,
  ) => void;
}

export function isFittedRenderRequestMessage(
  message: unknown,
): message is FittedRenderRequestMessage {
  if (typeof message !== "object" || message === null) {
    return false;
  }

  const candidate = message as Partial<FittedRenderRequestMessage>;
  return (
    candidate.version === PROTOCOL_VERSION &&
    candidate.type === FITTED_RENDER_REQUEST &&
    typeof candidate.seq === "number" &&
    typeof candidate.input === "string" &&
    typeof candidate.format === "string" &&
    typeof candidate.configJson === "string" &&
    typeof candidate.fitJson === "string"
  );
}

export function createFittedRenderHandler(
  options: FittedRenderHandlerOptions,
): (message: FittedRenderRequestMessage) => Promise<void> {
  let modulePromise: Promise<FittedRenderWasmModule> | null = null;

  const getWasmModule = (): Promise<FittedRenderWasmModule> => {
    modulePromise ??= options.loadWasmModule().then(async (module) => {
      await module.default?.();
      return module;
    });
    return modulePromise;
  };

  return async (message) => {
    try {
      const module = await getWasmModule();
      const response = JSON.parse(
        module.renderFitted(
          message.input,
          message.format,
          message.configJson,
          message.fitJson,
        ),
      ) as { output: string; fit: FitReport };
      options.postMessage({
        version: PROTOCOL_VERSION,
        type: FITTED_RENDER_RESULT,
        seq: message.seq,
        format: message.format,
        output: response.output,
        fit: response.fit,
      });
    } catch (error) {
      options.postMessage({
        version: PROTOCOL_VERSION,
        type: "error",
        seq: message.seq,
        error: error instanceof Error ? error.message : String(error),
      });
    }
  };
}
