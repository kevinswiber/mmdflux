import { describe, expect, it, vi } from "vitest";
import type { MockWasmModule } from "./__test-fixtures__/wasm-module";
import { BrowserTextMetricsCapabilityError } from "./browser-text-metrics";
import { createWorkerRequestHandler } from "./worker";
import type {
  WorkerRequestMessage,
  WorkerResponseMessage,
} from "./worker-protocol";

describe("createWorkerRequestHandler", () => {
  it("initializes worker once and returns render results", async () => {
    const initialize = vi.fn(async () => {});
    const render = vi.fn(
      (input: string, format: string, configJson: string) => {
        return `${format}:${input}:${configJson}`;
      },
    );
    const validate = vi.fn(
      (input: string) => `{"valid":true,"input":"${input}"}`,
    );
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmModule> => ({
        default: initialize,
        browserTextMetricsRequest: () => "unused",
        render,
        renderWithBrowserTextMetrics: () => "unused",
        validate,
      }),
    );

    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      postMessage: (message) => responses.push(message),
    });

    const first: WorkerRequestMessage = {
      type: "render",
      seq: 1,
      input: "graph TD\nA-->B",
      format: "text",
      configJson: "{}",
    };
    const second: WorkerRequestMessage = {
      type: "render",
      seq: 2,
      input: "graph TD\nB-->C",
      format: "svg",
      configJson: '{"padding":2}',
    };

    await handler(first);
    await handler(second);

    expect(loadWasmModule).toHaveBeenCalledTimes(1);
    expect(initialize).toHaveBeenCalledTimes(1);
    expect(render).toHaveBeenCalledTimes(2);
    expect(validate).not.toHaveBeenCalled();
    expect(responses).toEqual([
      {
        type: "result",
        seq: 1,
        format: "text",
        output: "text:graph TD\nA-->B:{}",
      },
      {
        type: "result",
        seq: 2,
        format: "svg",
        output: 'svg:graph TD\nB-->C:{"padding":2}',
      },
    ]);
  });

  it("returns structured error payload on render failure", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmModule> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => {
          throw new Error("unknown output format: bad");
        },
        renderWithBrowserTextMetrics: () => "unused",
        validate: () => '{"valid":true}',
      }),
    );

    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      postMessage: (message) => responses.push(message),
    });

    const request: WorkerRequestMessage = {
      type: "render",
      seq: 42,
      input: "graph TD\nA-->B",
      format: "text",
      configJson: "{}",
    };

    await handler(request);

    expect(responses).toHaveLength(1);
    expect(responses[0]).toEqual({
      type: "error",
      seq: 42,
      error: "unknown output format: bad",
    });
  });

  it("returns validation results without reinitializing wasm", async () => {
    const initialize = vi.fn(async () => {});
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmModule> => ({
        default: initialize,
        browserTextMetricsRequest: () => "unused",
        render: () => "unused",
        renderWithBrowserTextMetrics: () => "unused",
        validate: (input) =>
          JSON.stringify({
            valid: input.includes("A-->B"),
            diagnostics: input.includes("A-->B")
              ? []
              : [{ message: "expected edge" }],
          }),
      }),
    );

    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      postMessage: (message) => responses.push(message),
    });

    const request: WorkerRequestMessage = {
      type: "validate",
      seq: -1,
      input: "graph TD\nA-->B",
    };

    await handler(request);

    expect(loadWasmModule).toHaveBeenCalledTimes(1);
    expect(initialize).toHaveBeenCalledTimes(1);
    expect(responses).toEqual([
      {
        type: "validation",
        seq: -1,
        resultJson: '{"valid":true,"diagnostics":[]}',
      },
    ]);
  });

  it("returns browser text metrics decisions without rendering or measuring", async () => {
    const browserTextMetricsRequest = vi.fn(
      (input: string, format: string, configJson: string) =>
        JSON.stringify({
          required: true,
          browserTextMetrics: {
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
          },
          input,
          format,
          configJson,
        }),
    );
    const render = vi.fn(() => "static unused");
    const renderWithBrowserTextMetrics = vi.fn(() => "dynamic unused");
    const prepareBrowserTextMetrics = vi.fn(async () => {
      throw new Error("should not prepare metrics for resolver");
    });
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmModule> => ({
        default: async () => {},
        browserTextMetricsRequest,
        render,
        renderWithBrowserTextMetrics,
        validate: () => '{"valid":true}',
      }),
    );

    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      prepareBrowserTextMetrics,
      postMessage: (message) => responses.push(message),
    });

    await handler({
      type: "resolveBrowserTextMetrics",
      seq: 8,
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: "{}",
    });

    expect(browserTextMetricsRequest).toHaveBeenCalledWith(
      "graph TD\nA-->B",
      "svg",
      "{}",
    );
    expect(render).not.toHaveBeenCalled();
    expect(renderWithBrowserTextMetrics).not.toHaveBeenCalled();
    expect(prepareBrowserTextMetrics).not.toHaveBeenCalled();
    expect(responses).toEqual([
      {
        type: "browserTextMetricsDecision",
        seq: 8,
        decision: {
          required: true,
          browserTextMetrics: {
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
          },
          input: "graph TD\nA-->B",
          format: "svg",
          configJson: "{}",
        },
      },
    ]);
  });

  it("returns structured errors for browser text metrics resolver failures", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmModule> => ({
        default: async () => {},
        browserTextMetricsRequest: () => {
          throw new Error("RenderConfig.graph_text_style is not supported");
        },
        render: () => "static unused",
        renderWithBrowserTextMetrics: () => "dynamic unused",
        validate: () => '{"valid":true}',
      }),
    );

    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      postMessage: (message) => responses.push(message),
    });

    await handler({
      type: "resolveBrowserTextMetrics",
      seq: 15,
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: '{"fontFamily":"Inter","fontSize":16}',
    });

    expect(responses).toEqual([
      {
        type: "error",
        seq: 15,
        error: "RenderConfig.graph_text_style is not supported",
      },
    ]);
  });

  it("prepares browser metrics and invokes the dynamic wasm export", async () => {
    const measureText = vi.fn(() => 42);
    const prepareBrowserTextMetrics = vi.fn(async () => ({
      metricsJson: '{"cssFont":"16px Inter"}',
      measureText,
    }));
    const renderWithBrowserTextMetrics = vi.fn(
      (
        input: string,
        format: string,
        configJson: string,
        metricsJson: string,
        callback: (text: string, cssFont: string) => number,
      ) =>
        `${format}:${input}:${configJson}:${metricsJson}:${callback("A", "font")}`,
    );
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmModule> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => "static unused",
        renderWithBrowserTextMetrics,
        validate: () => '{"valid":true}',
      }),
    );

    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      prepareBrowserTextMetrics,
      postMessage: (message) => responses.push(message),
    });

    await handler({
      type: "renderWithBrowserTextMetrics",
      seq: 9,
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: "{}",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    });

    expect(prepareBrowserTextMetrics).toHaveBeenCalledWith({
      fontFamily: "Inter",
      fontSizePx: 16,
      lineHeightPx: 24,
    });
    expect(renderWithBrowserTextMetrics).toHaveBeenCalledWith(
      "graph TD\nA-->B",
      "svg",
      "{}",
      '{"cssFont":"16px Inter"}',
      measureText,
    );
    expect(responses).toEqual([
      {
        type: "result",
        seq: 9,
        format: "svg",
        output: 'svg:graph TD\nA-->B:{}:{"cssFont":"16px Inter"}:42',
      },
    ]);
  });

  it("returns structured errors for dynamic metric preparation failures", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmModule> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => "static unused",
        renderWithBrowserTextMetrics: () => "dynamic unused",
        validate: () => '{"valid":true}',
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      prepareBrowserTextMetrics: vi.fn(async () => {
        throw new Error("Dynamic text metrics require OffscreenCanvas");
      }),
      postMessage: (message) => responses.push(message),
    });

    await handler({
      type: "renderWithBrowserTextMetrics",
      seq: 10,
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: "{}",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    });

    expect(responses).toEqual([
      {
        type: "error",
        seq: 10,
        error: "Dynamic text metrics require OffscreenCanvas",
      },
    ]);
  });

  it("codes dynamic metric capability errors without leaking adapter subcodes", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmModule> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => "static unused",
        renderWithBrowserTextMetrics: () => "dynamic unused",
        validate: () => '{"valid":true}',
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      prepareBrowserTextMetrics: vi.fn(async () => {
        throw new BrowserTextMetricsCapabilityError({
          code: "worker-offscreen-canvas-unavailable",
          message:
            "Dynamic text metrics require OffscreenCanvas in the worker.",
        });
      }),
      postMessage: (message) => responses.push(message),
    });

    await handler({
      type: "renderWithBrowserTextMetrics",
      seq: 11,
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: "{}",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    });

    expect(responses).toEqual([
      {
        type: "error",
        seq: 11,
        error: "Dynamic text metrics require OffscreenCanvas in the worker.",
        code: "dynamic-metrics-capability",
      },
    ]);
    expect(responses[0]).not.toHaveProperty(
      "code",
      "worker-offscreen-canvas-unavailable",
    );
    expect(JSON.stringify(responses[0])).not.toContain(
      "worker-offscreen-canvas-unavailable",
    );
  });

  it("does not code fallback-ineligible dynamic metric capability errors", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmModule> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => "static unused",
        renderWithBrowserTextMetrics: () => "dynamic unused",
        validate: () => '{"valid":true}',
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      prepareBrowserTextMetrics: vi.fn(async () => {
        throw new BrowserTextMetricsCapabilityError({
          code: "main-thread-font-face-set-unavailable",
          message:
            "Dynamic text metrics require document.fonts on the main thread.",
          fallbackEligible: false,
        });
      }),
      postMessage: (message) => responses.push(message),
    });

    await handler({
      type: "renderWithBrowserTextMetrics",
      seq: 12,
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: "{}",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    });

    expect(responses).toEqual([
      {
        type: "error",
        seq: 12,
        error:
          "Dynamic text metrics require document.fonts on the main thread.",
      },
    ]);
  });
});
