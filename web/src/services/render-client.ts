import type {
  WorkerOutputFormat,
  WorkerRequestMessage,
  WorkerResponseMessage,
} from "../worker-protocol";

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

interface PendingRenderRequest {
  kind: "render";
  resolve: (response: RenderResponse) => void;
  reject: (error: Error) => void;
}

interface PendingValidateRequest {
  kind: "validate";
  resolve: (resultJson: string) => void;
  reject: (error: Error) => void;
}

type PendingRequest = PendingRenderRequest | PendingValidateRequest;

export interface RenderWorkerClient {
  render: (request: RenderRequest) => Promise<RenderResponse>;
  validate: (input: string) => Promise<string>;
  terminate: () => void;
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
): RenderWorkerClient {
  const pending = new Map<number, PendingRequest>();
  let nextValidationSeq = -1;

  worker.onmessage = (event: MessageEvent<WorkerResponseMessage>) => {
    const response = event.data;
    const pendingRequest = pending.get(response.seq);
    if (!pendingRequest) {
      return;
    }

    pending.delete(response.seq);

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

    pendingRequest.reject(new Error(response.error));
  };

  return {
    render: (request) => {
      const currentSeq = request.seq;

      return new Promise<RenderResponse>((resolve, reject) => {
        const message: WorkerRequestMessage = {
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
    validate: (input) => {
      const seq = nextValidationSeq;
      nextValidationSeq -= 1;

      return new Promise<string>((resolve, reject) => {
        const message: WorkerRequestMessage = {
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
