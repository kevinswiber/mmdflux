import type {
  BrowserTextMetricsEnvironment,
  BrowserTextMetricsRequest,
  MainThreadBrowserTextMetricsEnvironment,
} from "./prepare.js";
import type {
  WorkerRequestMessage,
  WorkerResponseMessage,
} from "./worker-protocol.js";

// --- Spy factory (no vitest dependency) ---

export interface Spy<Args extends unknown[], R> {
  (...args: Args): R;
  readonly calls: Args[];
}

export function spy<Args extends unknown[], R>(
  impl: (...args: Args) => R,
): Spy<Args, R> {
  const calls: Args[] = [];
  // Use a `function` expression (not an arrow) so callers that bind `this`
  // — e.g. `document.createElement(...)` — keep their receiver intact.
  const fn = function (this: unknown, ...args: Args): R {
    calls.push(args);
    return impl.apply(this, args);
  } as Spy<Args, R>;
  Object.defineProperty(fn, "calls", { value: calls });
  return fn;
}

// --- FontFaceSet fixture ---

export interface FontFaceSetLike {
  load: Spy<[cssFont: string], Promise<unknown[]>>;
  ready?: Promise<unknown>;
  check: Spy<[cssFont: string], boolean>;
}

export function fontSetFixture(
  overrides: {
    load?: FontFaceSetLike["load"];
    check?: FontFaceSetLike["check"];
    ready?: Promise<unknown>;
  } = {},
): FontFaceSetLike {
  return {
    load: overrides.load ?? spy(async (_cssFont: string) => [{}]),
    check: overrides.check ?? spy((_cssFont: string) => true),
    ready: "ready" in overrides ? overrides.ready : Promise.resolve(),
  };
}

// --- Canvas + environment fixtures ---

interface FakeCanvasContext {
  font: string;
  measureText: Spy<[text: string], { width: number }>;
}

export function environmentFixture(
  fonts: FontFaceSetLike | undefined = fontSetFixture(),
): { context: FakeCanvasContext; environment: BrowserTextMetricsEnvironment } {
  const context: FakeCanvasContext = {
    font: "",
    measureText: spy((text: string) => ({ width: text.length * 3 })),
  };
  class FakeOffscreenCanvas {
    getContext(type: string): FakeCanvasContext | null {
      return type === "2d" ? context : null;
    }
  }
  const environment: BrowserTextMetricsEnvironment = {
    OffscreenCanvas: FakeOffscreenCanvas,
    fonts,
  };
  return { context, environment };
}

export function mainThreadEnvironmentFixture(
  fonts: FontFaceSetLike | undefined = fontSetFixture(),
): {
  context: FakeCanvasContext;
  environment: MainThreadBrowserTextMetricsEnvironment;
} {
  const context: FakeCanvasContext = {
    font: "",
    measureText: spy((text: string) => ({ width: text.length * 3 })),
  };
  const canvas = {
    getContext: (type: "2d"): FakeCanvasContext | null =>
      type === "2d" ? context : null,
  };
  const document = {
    fonts,
    createElement: spy(function (
      this: unknown,
      tagName: "canvas",
    ): typeof canvas {
      if (this !== document) {
        throw new Error("createElement lost document receiver");
      }
      if (tagName !== "canvas") {
        throw new Error(`unexpected element: ${tagName}`);
      }
      return canvas;
    }),
  };
  const environment: MainThreadBrowserTextMetricsEnvironment = { document };
  return { context, environment };
}

// --- Multi-style request fixture ---

export function multiStyleRequest(): Required<
  Pick<BrowserTextMetricsRequest, "defaultStyle" | "textStyles">
> {
  return {
    defaultStyle: "s0",
    textStyles: [
      {
        id: "s0",
        fontFamily: "Inter",
        fontSize: 16,
        lineHeight: 24,
        fontStyle: "normal",
        fontWeight: "400",
      },
      {
        id: "s1",
        fontFamily: "Verdana",
        fontSize: 8,
        lineHeight: 12,
        fontStyle: "normal",
        fontWeight: "400",
      },
    ],
  };
}

// --- Mock worker fixture ---

export interface MockWorkerOptions {
  dynamicResponse?: WorkerResponseMessage;
  resolverResponse?: WorkerResponseMessage;
  suppressDynamicResponse?: boolean;
  throwOnDynamicPost?: boolean;
  throwOnResolverPost?: boolean;
}

export class MockWorker {
  onmessage: ((event: MessageEvent<WorkerResponseMessage>) => void) | null =
    null;
  messages: WorkerRequestMessage[] = [];

  constructor(private readonly options: MockWorkerOptions = {}) {}

  postMessage(message: WorkerRequestMessage): void {
    this.messages.push(message);
    if (
      message.type === "renderWithBrowserTextMetrics" &&
      this.options.throwOnDynamicPost
    ) {
      throw new Error("worker post failed");
    }
    if (
      message.type === "resolveBrowserTextMetrics" &&
      this.options.throwOnResolverPost
    ) {
      throw new Error("worker post failed");
    }
    if (!this.onmessage) {
      throw new Error("worker message handler was not installed");
    }

    queueMicrotask(() => {
      const { onmessage } = this;
      if (!onmessage) return;
      if (message.type === "render") {
        onmessage({
          data: {
            version: 1,
            type: "result",
            seq: message.seq,
            format: message.format,
            output: `${message.format}:${message.input}:${message.configJson}`,
          },
        } as MessageEvent<WorkerResponseMessage>);
        return;
      }

      if (message.type === "renderWithBrowserTextMetrics") {
        if (this.options.suppressDynamicResponse) return;
        if (this.options.dynamicResponse) {
          onmessage({
            data: this.options.dynamicResponse,
          } as MessageEvent<WorkerResponseMessage>);
          return;
        }
        onmessage({
          data: {
            version: 1,
            type: "result",
            seq: message.seq,
            format: message.format,
            output: `${message.format}:${message.input}:${message.configJson}:${message.browserTextMetrics.fontFamily}`,
          },
        } as MessageEvent<WorkerResponseMessage>);
        return;
      }

      if (message.type === "resolveBrowserTextMetrics") {
        if (this.options.resolverResponse) {
          onmessage({
            data: this.options.resolverResponse,
          } as MessageEvent<WorkerResponseMessage>);
          return;
        }
        const required = message.input.includes("font-family");
        onmessage({
          data: {
            version: 1,
            type: "browserTextMetricsDecision",
            seq: message.seq,
            decision: {
              required,
              browserTextMetrics: required
                ? {
                    defaultStyle: "s0",
                    textStyles: [
                      {
                        id: "s0",
                        fontFamily: "Verdana",
                        fontSize: 8,
                        fontStyle: "normal",
                        fontWeight: "400",
                        lineHeight: 12,
                        cssFont: "8px Verdana",
                      },
                    ],
                  }
                : undefined,
            },
          },
        } as MessageEvent<WorkerResponseMessage>);
        return;
      }

      onmessage({
        data: {
          version: 1,
          type: "validation",
          seq: message.seq,
          resultJson: '{"valid":true}',
        },
      } as MessageEvent<WorkerResponseMessage>);
    });
  }

  terminate(): void {
    // no-op
  }
}

export function createMockRenderWorker(
  options: MockWorkerOptions = {},
): MockWorker {
  return new MockWorker(options);
}

// --- Wasm module fixture ---

export interface MockWasmModule {
  default: Spy<[], Promise<void>>;
  browserTextMetricsRequest: Spy<
    [input: string, format: string, configJson: string],
    string
  >;
  render: Spy<[input: string, format: string, configJson: string], string>;
  renderWithBrowserTextMetrics: Spy<
    [
      input: string,
      format: string,
      configJson: string,
      metricsJson: string,
      measureText: (text: string, cssFont: string) => number,
    ],
    string
  >;
  validate: Spy<[input: string], string>;
}

export interface WasmModuleFixture {
  initialize: MockWasmModule["default"];
  module: MockWasmModule;
  browserTextMetricsRequest: MockWasmModule["browserTextMetricsRequest"];
  render: MockWasmModule["render"];
  renderWithBrowserTextMetrics: MockWasmModule["renderWithBrowserTextMetrics"];
  validate: MockWasmModule["validate"];
}

export function wasmModuleFixture(
  renderWithBrowserTextMetrics: MockWasmModule["renderWithBrowserTextMetrics"] = spy(
    (
      input: string,
      format: string,
      configJson: string,
      metricsJson: string,
      callback: (text: string, cssFont: string) => number,
    ) =>
      `${format}:${input}:${configJson}:${metricsJson}:${callback("A", "font")}`,
  ),
): WasmModuleFixture {
  const initialize = spy(async () => undefined);
  const browserTextMetricsRequest = spy(
    (_input: string, _format: string, _configJson: string) =>
      '{"required":false}',
  );
  const render = spy(
    (_input: string, _format: string, _configJson: string) => "static unused",
  );
  const validate = spy((_input: string) => '{"valid":true}');
  const module: MockWasmModule = {
    default: initialize as unknown as MockWasmModule["default"],
    browserTextMetricsRequest,
    render,
    renderWithBrowserTextMetrics,
    validate,
  };
  return {
    initialize: initialize as unknown as MockWasmModule["default"],
    module,
    browserTextMetricsRequest,
    render,
    renderWithBrowserTextMetrics,
    validate,
  };
}
