import type { BrowserTextMetricsRequest } from "../browser-text-metrics";
import {
  PROTOCOL_VERSION,
  type WorkerBrowserTextMetricsDecision,
  type WorkerOutputFormat,
  type WorkerRequestMessage,
  type WorkerResponseMessage,
} from "../worker-protocol";
import type { MainThreadBrowserTextMetricsRenderer } from "./main-thread-browser-text-metrics";

export interface RenderRequest {
  seq: number;
  input: string;
  format: WorkerOutputFormat;
  configJson?: string;
}

export interface RenderResponse {
  seq: number;
  format: WorkerOutputFormat;
  output: string;
}

export interface BrowserTextMetricsRenderRequest {
  seq: number;
  input: string;
  configJson?: string;
  browserTextMetrics: BrowserTextMetricsRequest;
}

export interface BrowserTextMetricsDecisionRequest {
  seq: number;
  input: string;
  format: WorkerOutputFormat;
  configJson?: string;
}

interface PendingRenderRequest {
  kind: "render";
  resolve: (response: RenderResponse) => void;
  reject: (error: Error) => void;
  mainThreadFallback?: () => Promise<RenderResponse>;
  timeoutHandle?: ReturnType<typeof setTimeout>;
}

interface PendingValidateRequest {
  kind: "validate";
  resolve: (resultJson: string) => void;
  reject: (error: Error) => void;
}

interface PendingBrowserTextMetricsDecisionRequest {
  kind: "resolveBrowserTextMetrics";
  resolve: (decision: WorkerBrowserTextMetricsDecision) => void;
  reject: (error: Error) => void;
}

type PendingRequest =
  | PendingRenderRequest
  | PendingValidateRequest
  | PendingBrowserTextMetricsDecisionRequest;

export interface RenderWorkerClient {
  render: (request: RenderRequest) => Promise<RenderResponse>;
  renderWithBrowserTextMetrics: (
    request: BrowserTextMetricsRenderRequest,
  ) => Promise<RenderResponse>;
  resolveBrowserTextMetricsRequest: (
    request: BrowserTextMetricsDecisionRequest,
  ) => Promise<WorkerBrowserTextMetricsDecision>;
  validate: (input: string) => Promise<string>;
  terminate: () => void;
}

export interface RenderWorkerClientOptions {
  mainThreadBrowserTextMetricsRenderer?: MainThreadBrowserTextMetricsRenderer;
  dynamicMetricsWorkerTimeoutMs?: number;
}

function createDefaultWorker(): Worker {
  return new Worker(new URL("../worker.ts", import.meta.url), {
    type: "module",
  });
}

function toMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function createRenderWorkerClient(
  worker: Worker = createDefaultWorker(),
  options: RenderWorkerClientOptions = {},
): RenderWorkerClient {
  const pending = new Map<number, PendingRequest>();
  const mainThreadRenderer = options.mainThreadBrowserTextMetricsRenderer;
  const dynamicMetricsWorkerTimeoutMs =
    options.dynamicMetricsWorkerTimeoutMs ?? 5_000;
  let nextValidationSeq = -1;

  worker.onmessage = (event: MessageEvent<WorkerResponseMessage>) => {
    const response = event.data;
    const pendingRequest = pending.get(response.seq);
    if (!pendingRequest) {
      return;
    }

    pending.delete(response.seq);
    if (pendingRequest.kind === "render" && pendingRequest.timeoutHandle) {
      clearTimeout(pendingRequest.timeoutHandle);
    }

    if (response.type === "result") {
      if (pendingRequest.kind !== "render") {
        pendingRequest.reject(
          new Error("worker returned render output for a validation request"),
        );
        return;
      }

      pendingRequest.resolve({
        seq: response.seq,
        format: response.format,
        output: response.output,
      });
      return;
    }

    if (response.type === "browserTextMetricsDecision") {
      if (pendingRequest.kind !== "resolveBrowserTextMetrics") {
        pendingRequest.reject(
          new Error(
            "worker returned browser metrics output for another request",
          ),
        );
        return;
      }

      pendingRequest.resolve(response.decision);
      return;
    }

    if (response.type === "validation") {
      if (pendingRequest.kind !== "validate") {
        pendingRequest.reject(
          new Error("worker returned validation output for a render request"),
        );
        return;
      }

      pendingRequest.resolve(response.resultJson);
      return;
    }

    if (
      pendingRequest.kind === "render" &&
      response.code === "dynamic-metrics-capability" &&
      pendingRequest.mainThreadFallback
    ) {
      pendingRequest
        .mainThreadFallback()
        .then(pendingRequest.resolve, pendingRequest.reject);
      return;
    }

    pendingRequest.reject(new Error(response.error));
  };

  return {
    render: (request) => {
      const currentSeq = request.seq;

      return new Promise<RenderResponse>((resolve, reject) => {
        const message: WorkerRequestMessage = {
          version: PROTOCOL_VERSION,
          type: "render",
          seq: currentSeq,
          input: request.input,
          format: request.format,
          configJson: request.configJson ?? "{}",
        };

        pending.set(currentSeq, { kind: "render", resolve, reject });

        try {
          worker.postMessage(message);
        } catch (error) {
          pending.delete(currentSeq);
          reject(
            new Error(`failed to post render request: ${toMessage(error)}`),
          );
        }
      });
    },
    resolveBrowserTextMetricsRequest: (request) => {
      const currentSeq = request.seq;

      return new Promise<WorkerBrowserTextMetricsDecision>(
        (resolve, reject) => {
          const message: WorkerRequestMessage = {
            version: PROTOCOL_VERSION,
            type: "resolveBrowserTextMetrics",
            seq: currentSeq,
            input: request.input,
            format: request.format,
            configJson: request.configJson ?? "{}",
          };

          pending.set(currentSeq, {
            kind: "resolveBrowserTextMetrics",
            resolve,
            reject,
          });

          try {
            worker.postMessage(message);
          } catch (error) {
            pending.delete(currentSeq);
            reject(
              new Error(
                `failed to post browser text metrics request: ${toMessage(error)}`,
              ),
            );
          }
        },
      );
    },
    renderWithBrowserTextMetrics: (request) => {
      const currentSeq = request.seq;

      return new Promise<RenderResponse>((resolve, reject) => {
        const mainThreadFallback = mainThreadRenderer
          ? () => mainThreadRenderer.renderWithBrowserTextMetrics(request)
          : undefined;
        const pendingRequest: PendingRenderRequest = {
          kind: "render",
          resolve,
          reject,
          mainThreadFallback,
        };
        const message: WorkerRequestMessage = {
          version: PROTOCOL_VERSION,
          type: "renderWithBrowserTextMetrics",
          seq: currentSeq,
          input: request.input,
          format: "svg",
          configJson: request.configJson ?? "{}",
          browserTextMetrics: request.browserTextMetrics,
        };

        if (
          mainThreadFallback &&
          Number.isFinite(dynamicMetricsWorkerTimeoutMs) &&
          dynamicMetricsWorkerTimeoutMs > 0
        ) {
          pendingRequest.timeoutHandle = setTimeout(() => {
            if (pending.get(currentSeq) !== pendingRequest) {
              return;
            }

            pending.delete(currentSeq);
            mainThreadFallback().then(resolve, reject);
          }, dynamicMetricsWorkerTimeoutMs);
        }

        pending.set(currentSeq, pendingRequest);

        try {
          worker.postMessage(message);
        } catch (error) {
          if (pendingRequest.timeoutHandle) {
            clearTimeout(pendingRequest.timeoutHandle);
          }
          pending.delete(currentSeq);
          reject(
            new Error(
              `failed to post dynamic render request: ${toMessage(error)}`,
            ),
          );
        }
      });
    },
    validate: (input) => {
      const seq = nextValidationSeq;
      nextValidationSeq -= 1;

      return new Promise<string>((resolve, reject) => {
        const message: WorkerRequestMessage = {
          version: PROTOCOL_VERSION,
          type: "validate",
          seq,
          input,
        };

        pending.set(seq, { kind: "validate", resolve, reject });

        try {
          worker.postMessage(message);
        } catch (error) {
          pending.delete(seq);
          reject(
            new Error(`failed to post validation request: ${toMessage(error)}`),
          );
        }
      });
    },
    terminate: () => {
      worker.terminate();
      for (const request of pending.values()) {
        request.reject(new Error("render worker terminated"));
      }
      pending.clear();
    },
  };
}
