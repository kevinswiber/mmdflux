import { vi } from "vitest";
import type {
  BrowserTextMetricsEnvironment,
  MainThreadBrowserTextMetricsEnvironment,
} from "../src/prepare.js";
import type {
  WorkerRequestMessage,
  WorkerResponseMessage,
} from "../src/worker-protocol.js";

export interface FakeTextMetrics {
  width: number;
}

export interface FakeCanvasContext {
  font: string;
  measureText: (text: string) => FakeTextMetrics;
}

export interface FakeFontFaceSet {
  load: (cssFont: string) => Promise<unknown[]>;
  ready: Promise<unknown>;
  check: (cssFont: string) => boolean;
}

export function fontSetFixture(
  overrides: Partial<FakeFontFaceSet> = {},
): FakeFontFaceSet {
  return {
    load: vi.fn(async () => [{}]),
    ready: Promise.resolve(),
    check: vi.fn(() => true),
    ...overrides,
  };
}

export function environmentFixture(
  fonts: FakeFontFaceSet | undefined = fontSetFixture(),
) {
  const context: FakeCanvasContext = {
    font: "",
    measureText: vi.fn((text: string) => ({ width: text.length * 3 })),
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
  fonts: FakeFontFaceSet | undefined = fontSetFixture(),
) {
  const context: FakeCanvasContext = {
    font: "",
    measureText: vi.fn((text: string) => ({ width: text.length * 3 })),
  };
  const canvas = {
    getContext: (type: "2d"): FakeCanvasContext | null =>
      type === "2d" ? context : null,
  };
  const document = {
    fonts,
    createElement: vi.fn(function (
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

export function multiStyleRequest() {
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

export interface MockWasmModule {
  default: () => Promise<void>;
  browserTextMetricsRequest: (
    input: string,
    format: string,
    configJson: string,
  ) => string;
  render: (input: string, format: string, configJson: string) => string;
  renderWithBrowserTextMetrics: (
    input: string,
    format: string,
    configJson: string,
    metricsJson: string,
    measureText: (text: string, cssFont: string) => number,
  ) => string;
  validate: (input: string) => string;
}

export function wasmModuleFixture(
  renderWithBrowserTextMetrics = vi.fn(
    (
      input: string,
      format: string,
      configJson: string,
      metricsJson: string,
      callback: (text: string, cssFont: string) => number,
    ) =>
      `${format}:${input}:${configJson}:${metricsJson}:${callback("A", "font")}`,
  ),
) {
  const initialize = vi.fn(async () => {});
  const browserTextMetricsRequest = vi.fn(() => '{"required":false}');
  const render = vi.fn(() => "static unused");
  const validate = vi.fn(() => '{"valid":true}');
  const module: MockWasmModule = {
    default: initialize,
    browserTextMetricsRequest,
    render,
    renderWithBrowserTextMetrics,
    validate,
  };

  return {
    initialize,
    module,
    browserTextMetricsRequest,
    render,
    renderWithBrowserTextMetrics,
    validate,
  };
}

export interface MockRenderWorkerOptions {
  dynamicResponse?: WorkerResponseMessage;
  resolverResponse?: WorkerResponseMessage;
  suppressDynamicResponse?: boolean;
  throwOnDynamicPost?: boolean;
  throwOnResolverPost?: boolean;
}

export class MockRenderWorker {
  onmessage: ((event: MessageEvent<WorkerResponseMessage>) => void) | null =
    null;
  messages: WorkerRequestMessage[] = [];

  constructor(private readonly options: MockRenderWorkerOptions = {}) {}

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
      if (message.type === "render") {
        this.onmessage?.({
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
          this.onmessage?.({
            data: { ...this.options.dynamicResponse, seq: message.seq },
          } as MessageEvent<WorkerResponseMessage>);
          return;
        }
        this.onmessage?.({
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
          this.onmessage?.({
            data: { ...this.options.resolverResponse, seq: message.seq },
          } as MessageEvent<WorkerResponseMessage>);
          return;
        }
        this.onmessage?.({
          data: {
            version: 1,
            type: "browserTextMetricsDecision",
            seq: message.seq,
            decision: {
              required: message.input.includes("font-family"),
              browserTextMetrics: message.input.includes("font-family")
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

      this.onmessage?.({
        data: {
          version: 1,
          type: "validation",
          seq: message.seq,
          resultJson: '{"valid":true}',
        },
      } as MessageEvent<WorkerResponseMessage>);
    });
  }

  terminate(): void {}
}

export function mainThreadRendererFixture(output = "main-thread-svg") {
  return {
    renderSvg: vi.fn(
      async (request: {
        input: string;
        browserTextMetrics: unknown;
        configJson?: string;
      }) => ({
        output: `${output}:${request.input}`,
        format: "svg" as const,
        source: "main-thread" as const,
      }),
    ),
  };
}
