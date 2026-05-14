import {
  MmdsBrowserTextMetricsCapabilityError,
  type MmdsBrowserTextMetricsErrorCode,
} from "./capability.js";
import type {
  MmdsDynamicRenderOptions,
  MmdsMainThreadRenderer,
  MmdsRenderFormat,
  MmdsRenderResult,
  MmdsStaticRenderOptions,
} from "./main-thread.js";
import { mayNeedBrowserTextMetrics } from "./routing.js";
import {
  isWorkerRequestMessage,
  PROTOCOL_VERSION,
  type WorkerBrowserTextMetricsDecision,
  type WorkerErrorMessage,
  type WorkerRequestMessage,
  type WorkerResponseMessage,
} from "./worker-protocol.js";

export type { MmdsRenderFormat, MmdsRenderResult } from "./main-thread.js";

/**
 * The surface the client actually consumes from a `Worker`. Narrowing the
 * option to this interface (rather than DOM `Worker`) means fixtures and
 * non-DOM hosts (Comlink wrappers, partytown, hand-rolled bridges) are
 * type-compatible without casts. A real `Worker` satisfies this implicitly
 * because the methods are a subset.
 */
export interface MmdsWorkerLike {
  onmessage: ((event: MessageEvent<WorkerResponseMessage>) => void) | null;
  postMessage(message: WorkerRequestMessage): void;
  terminate(): void;
}

export interface MmdsBrowserTextMetricsClientOptions {
  readonly worker: MmdsWorkerLike;
  readonly fallback?: MmdsMainThreadRenderer;
  /** Default 5000ms. Set to 0 or non-finite to disable the timeout. */
  readonly dynamicMetricsWorkerTimeoutMs?: number;
}

export interface MmdsBrowserTextMetricsClient {
  renderSvg(options: MmdsDynamicRenderOptions): Promise<MmdsRenderResult>;
  renderStatic(options: MmdsStaticRenderOptions): Promise<MmdsRenderResult>;
  validate(input: string): Promise<string>;
  resolveBrowserTextMetricsRequest(
    options: MmdsStaticRenderOptions,
  ): Promise<WorkerBrowserTextMetricsDecision>;
  renderAuto(options: MmdsStaticRenderOptions): Promise<MmdsRenderResult>;
  terminate(): void;
}

interface PendingRender {
  kind: "render";
  format: MmdsRenderFormat;
  resolve: (result: MmdsRenderResult) => void;
  reject: (error: Error) => void;
  mainThreadFallback?: () => Promise<MmdsRenderResult>;
  timeoutHandle?: ReturnType<typeof setTimeout>;
}

interface PendingValidate {
  kind: "validate";
  resolve: (resultJson: string) => void;
  reject: (error: Error) => void;
}

interface PendingResolve {
  kind: "resolveBrowserTextMetrics";
  resolve: (decision: WorkerBrowserTextMetricsDecision) => void;
  reject: (error: Error) => void;
}

type Pending = PendingRender | PendingValidate | PendingResolve;

const DEFAULT_DYNAMIC_TIMEOUT_MS = 5_000;

export function createMmdsBrowserTextMetricsClient(
  options: MmdsBrowserTextMetricsClientOptions,
): MmdsBrowserTextMetricsClient {
  const { worker } = options;
  const fallback = options.fallback;
  const dynamicTimeoutMs =
    options.dynamicMetricsWorkerTimeoutMs ?? DEFAULT_DYNAMIC_TIMEOUT_MS;
  const pending = new Map<number, Pending>();
  let nextStaticSeq = 1;
  let nextValidateSeq = -1;
  let nextResolveSeq = 1_000_000_000;
  let nextDynamicSeq = 2_000_000_000;

  worker.onmessage = (event) => {
    const response = event.data;
    const slot = pending.get(response.seq);
    if (!slot) return;
    pending.delete(response.seq);
    if (slot.kind === "render" && slot.timeoutHandle) {
      clearTimeout(slot.timeoutHandle);
    }

    if (response.type === "result") {
      if (slot.kind !== "render") {
        slot.reject(new Error("worker returned a result for a non-render seq"));
        return;
      }
      slot.resolve({
        output: response.output,
        format: response.format,
        source: "worker",
      });
      return;
    }

    if (response.type === "validation") {
      if (slot.kind !== "validate") {
        slot.reject(
          new Error("worker returned validation for a non-validate seq"),
        );
        return;
      }
      slot.resolve(response.resultJson);
      return;
    }

    if (response.type === "browserTextMetricsDecision") {
      if (slot.kind !== "resolveBrowserTextMetrics") {
        slot.reject(
          new Error("worker returned a decision for a non-resolver seq"),
        );
        return;
      }
      slot.resolve(response.decision);
      return;
    }

    // response.type === "error"
    if (
      slot.kind === "render" &&
      response.code === "dynamic-metrics-capability" &&
      slot.mainThreadFallback
    ) {
      slot.mainThreadFallback().then(slot.resolve, slot.reject);
      return;
    }
    slot.reject(rebuildError(response.error, response.code));
  };

  function postOrReject(
    seq: number,
    message: WorkerRequestMessage,
    reject: (e: Error) => void,
    onPostFailure: string,
  ) {
    try {
      worker.postMessage(message);
    } catch (error) {
      pending.delete(seq);
      reject(new Error(`${onPostFailure}: ${formatError(error)}`));
    }
  }

  return {
    renderSvg(opts) {
      const seq = nextDynamicSeq++;
      return new Promise<MmdsRenderResult>((resolve, reject) => {
        const mainThreadFallback = fallback
          ? () => fallback.renderSvg(opts)
          : undefined;
        const slot: PendingRender = {
          kind: "render",
          format: "svg",
          resolve,
          reject,
          mainThreadFallback,
        };
        if (Number.isFinite(dynamicTimeoutMs) && dynamicTimeoutMs > 0) {
          slot.timeoutHandle = setTimeout(() => {
            if (pending.get(seq) !== slot) return;
            pending.delete(seq);
            if (mainThreadFallback) {
              mainThreadFallback().then(resolve, reject);
              return;
            }
            reject(
              new Error(
                `dynamic render timed out after ${dynamicTimeoutMs}ms with no fallback configured`,
              ),
            );
          }, dynamicTimeoutMs);
        }
        pending.set(seq, slot);
        postOrReject(
          seq,
          {
            version: PROTOCOL_VERSION,
            type: "renderWithBrowserTextMetrics",
            seq,
            input: opts.input,
            format: "svg",
            configJson: opts.configJson ?? "{}",
            browserTextMetrics: opts.browserTextMetrics,
          },
          (error) => {
            if (slot.timeoutHandle) clearTimeout(slot.timeoutHandle);
            reject(error);
          },
          "failed to post dynamic render request",
        );
      });
    },
    renderStatic(opts) {
      const seq = nextStaticSeq++;
      return new Promise<MmdsRenderResult>((resolve, reject) => {
        pending.set(seq, {
          kind: "render",
          format: opts.format,
          resolve,
          reject,
        });
        postOrReject(
          seq,
          {
            version: PROTOCOL_VERSION,
            type: "render",
            seq,
            input: opts.input,
            format: opts.format,
            configJson: opts.configJson ?? "{}",
          },
          reject,
          "failed to post render request",
        );
      });
    },
    validate(input) {
      const seq = nextValidateSeq--;
      return new Promise<string>((resolve, reject) => {
        pending.set(seq, { kind: "validate", resolve, reject });
        postOrReject(
          seq,
          { version: PROTOCOL_VERSION, type: "validate", seq, input },
          reject,
          "failed to post validation request",
        );
      });
    },
    resolveBrowserTextMetricsRequest(opts) {
      const seq = nextResolveSeq++;
      return new Promise<WorkerBrowserTextMetricsDecision>(
        (resolve, reject) => {
          pending.set(seq, {
            kind: "resolveBrowserTextMetrics",
            resolve,
            reject,
          });
          postOrReject(
            seq,
            {
              version: PROTOCOL_VERSION,
              type: "resolveBrowserTextMetrics",
              seq,
              input: opts.input,
              format: opts.format,
              configJson: opts.configJson ?? "{}",
            },
            reject,
            "failed to post browser text metrics request",
          );
        },
      );
    },
    async renderAuto(opts) {
      if (
        opts.format !== "svg" ||
        !mayNeedBrowserTextMetrics({
          input: opts.input,
          configJson: opts.configJson,
        })
      ) {
        return this.renderStatic(opts);
      }
      const decision = await this.resolveBrowserTextMetricsRequest(opts);
      if (!decision.required) {
        return this.renderStatic(opts);
      }
      if (!decision.browserTextMetrics) {
        throw new MmdsBrowserTextMetricsCapabilityError({
          code: "invalid-text-metrics-request",
          message:
            "browser text metrics decision required metrics but omitted the request",
          fallbackEligible: false,
        });
      }
      return this.renderSvg({
        input: opts.input,
        configJson: opts.configJson,
        browserTextMetrics: decision.browserTextMetrics,
      });
    },
    terminate() {
      worker.terminate();
      for (const slot of pending.values()) {
        if (slot.kind === "render" && slot.timeoutHandle) {
          clearTimeout(slot.timeoutHandle);
        }
        slot.reject(new Error("render worker terminated"));
      }
      pending.clear();
    },
  };
}

/**
 * Opt-in convenience that constructs the default bundler-coupled Worker:
 * `new Worker(new URL("./worker.js", import.meta.url), { type: "module" })`.
 * The single place in the package that touches the Worker constructor.
 * Callers using Comlink, partytown, or a hand-rolled bootstrap should
 * construct their own Worker and pass it to `createMmdsBrowserTextMetricsClient`.
 */
export function createDefaultMmdsWorker(): Worker {
  return new Worker(new URL("./worker.js", import.meta.url), {
    type: "module",
  });
}

export interface AutoClientOptions {
  /**
   * REQUIRED to opt into worker mode. The package never silently constructs
   * a Worker — pass `createDefaultMmdsWorker` for the bundler-coupled
   * default, or your own factory. The factory may return a DOM `Worker` or
   * any `MmdsWorkerLike` (e.g. the test fixture from `./fixtures`).
   */
  readonly workerFactory?: () => MmdsWorkerLike;
  readonly mainThreadFactory?: () => MmdsMainThreadRenderer;
  readonly dynamicMetricsWorkerTimeoutMs?: number;
}

/**
 * Returns a worker-backed client when `typeof Worker !== "undefined"` AND
 * a `workerFactory` was provided; otherwise returns `mainThreadFactory()`
 * when supplied. Throws `MmdsBrowserTextMetricsCapabilityError`
 * (`code: "unsupported-format"`) only when neither path is viable.
 *
 * | Worker defined | workerFactory | mainThreadFactory | Result               |
 * | -------------- | ------------- | ----------------- | -------------------- |
 * | yes            | yes           | yes               | client (worker+fb)   |
 * | yes            | yes           | no                | client (worker-only) |
 * | yes            | no            | yes               | main-thread renderer |
 * | yes            | no            | no                | throws               |
 * | no             | yes           | yes               | main-thread renderer |
 * | no             | yes           | no                | throws               |
 * | no             | no            | yes               | main-thread renderer |
 * | no             | no            | no                | throws               |
 */
export function createAutoMmdsBrowserTextMetricsClient(
  options: AutoClientOptions,
): MmdsBrowserTextMetricsClient | MmdsMainThreadRenderer {
  const workerAvailable = typeof globalThis.Worker !== "undefined";
  if (workerAvailable && options.workerFactory) {
    return createMmdsBrowserTextMetricsClient({
      worker: options.workerFactory(),
      fallback: options.mainThreadFactory?.(),
      dynamicMetricsWorkerTimeoutMs: options.dynamicMetricsWorkerTimeoutMs,
    });
  }
  if (options.mainThreadFactory) {
    return options.mainThreadFactory();
  }
  throw new MmdsBrowserTextMetricsCapabilityError({
    code: "unsupported-format",
    message: "no viable client path: Worker undefined and no mainThreadFactory",
    fallbackEligible: false,
  });
}

function formatError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

const KNOWN_WIRE_CODES: ReadonlySet<MmdsBrowserTextMetricsErrorCode> =
  Object.freeze(
    new Set<MmdsBrowserTextMetricsErrorCode>([
      "dynamic-metrics-capability",
      "unsupported-format",
      "wasm-render-rejected",
      "wasm-config-rejected",
      "wasm-reentered",
    ]),
  );

/**
 * Reconstruct a typed MmdsBrowserTextMetricsCapabilityError when the worker
 * sent back a known capability code. Without this, worker-backed render
 * paths would lose the wasm classification codes the worker handler
 * emits and surface only the message string.
 */
function rebuildError(
  message: string,
  code: WorkerErrorMessage["code"],
): Error {
  if (code && KNOWN_WIRE_CODES.has(code as MmdsBrowserTextMetricsErrorCode)) {
    return new MmdsBrowserTextMetricsCapabilityError({
      code: code as MmdsBrowserTextMetricsErrorCode,
      message,
    });
  }
  return new Error(message);
}

// Re-exported so tests can confirm WorkerRequestMessage shapes round-trip
// through the discriminant-aware guard.
export { isWorkerRequestMessage };
