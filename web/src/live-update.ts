import type { WorkerOutputFormat } from "@mmds/browser-text-metrics/worker-protocol";

export interface LiveUpdateRequest {
  input: string;
  format: WorkerOutputFormat;
  configJson: string;
}

export interface LiveUpdateRenderRequest extends LiveUpdateRequest {
  seq: number;
}

export interface LiveUpdateRenderResult {
  seq: number;
  format: WorkerOutputFormat;
  output: string;
}

export type LiveUpdateDebounceSetting =
  | number
  | ((request: LiveUpdateRequest) => number);

interface LiveUpdateControllerOptions {
  debounceMs?: LiveUpdateDebounceSetting;
  render: (request: LiveUpdateRenderRequest) => Promise<LiveUpdateRenderResult>;
  onResult: (result: LiveUpdateRenderResult) => void;
  onError: (message: string) => void;
}

export interface LiveUpdateController {
  schedule: (request: LiveUpdateRequest) => void;
  cancel: () => void;
}

function toMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function normalizeDelay(delay: number): number {
  if (!Number.isFinite(delay)) {
    return 0;
  }

  if (delay < 0) {
    return 0;
  }

  return Math.floor(delay);
}

export function createLiveUpdateController(
  options: LiveUpdateControllerOptions,
): LiveUpdateController {
  const debounceSetting = options.debounceMs ?? 200;
  let nextSeq = 0;
  let latestScheduled: LiveUpdateRequest | null = null;
  let latestSeq = 0;
  let timeoutHandle: ReturnType<typeof setTimeout> | null = null;

  const triggerRender = (request: LiveUpdateRequest): void => {
    const seq = ++nextSeq;
    latestSeq = seq;

    void options
      .render({
        seq,
        input: request.input,
        format: request.format,
        configJson: request.configJson,
      })
      .then((result) => {
        if (result.seq !== latestSeq) {
          return;
        }
        options.onResult(result);
      })
      .catch((error) => {
        if (seq !== latestSeq) {
          return;
        }
        options.onError(toMessage(error));
      });
  };

  return {
    schedule: (request: LiveUpdateRequest) => {
      latestScheduled = request;
      const debounceMs = normalizeDelay(
        typeof debounceSetting === "function"
          ? debounceSetting(request)
          : debounceSetting,
      );

      if (timeoutHandle !== null) {
        clearTimeout(timeoutHandle);
      }

      if (debounceMs === 0) {
        timeoutHandle = null;
        triggerRender(request);
        return;
      }

      timeoutHandle = setTimeout(() => {
        timeoutHandle = null;

        if (!latestScheduled) {
          return;
        }

        triggerRender(latestScheduled);
      }, debounceMs);
    },
    cancel: () => {
      if (timeoutHandle !== null) {
        clearTimeout(timeoutHandle);
        timeoutHandle = null;
      }
    },
  };
}
