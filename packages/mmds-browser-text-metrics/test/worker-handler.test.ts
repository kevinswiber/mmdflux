import { describe, expect, it, vi } from "vitest";
import { MmdsBrowserTextMetricsCapabilityError } from "../src/capability.js";
import { createWorkerRequestHandler } from "../src/worker.js";
import {
  PROTOCOL_VERSION,
  type WorkerRequestMessage,
  type WorkerResponseMessage,
} from "../src/worker-protocol.js";

interface MockWasmExports {
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

function renderMsg(
  overrides: Partial<Extract<WorkerRequestMessage, { type: "render" }>> = {},
): Extract<WorkerRequestMessage, { type: "render" }> {
  return {
    version: PROTOCOL_VERSION,
    type: "render",
    seq: 1,
    input: "graph TD\nA-->B",
    format: "svg",
    configJson: "{}",
    ...overrides,
  };
}

function dynamicMsg(
  overrides: Partial<
    Extract<WorkerRequestMessage, { type: "renderWithBrowserTextMetrics" }>
  > = {},
): Extract<WorkerRequestMessage, { type: "renderWithBrowserTextMetrics" }> {
  return {
    version: PROTOCOL_VERSION,
    type: "renderWithBrowserTextMetrics",
    seq: 1,
    input: "graph TD\nA-->B",
    format: "svg",
    configJson: "{}",
    browserTextMetrics: {
      fontFamily: "Inter",
      fontSizePx: 16,
      lineHeightPx: 24,
    },
    ...overrides,
  };
}

describe("createWorkerRequestHandler", () => {
  it("Q6 §3.7 — routes render to wasm.render and initializes wasm once", async () => {
    const initialize = vi.fn(async () => {});
    const render = vi.fn(
      (input: string, format: string, configJson: string) =>
        `${format}:${input}:${configJson}`,
    );
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmExports> => ({
        default: initialize,
        browserTextMetricsRequest: () => "unused",
        render,
        renderWithBrowserTextMetrics: () => "unused",
        validate: () => "unused",
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      postMessage: (m) => responses.push(m),
    });

    await handler(renderMsg({ seq: 1 }));
    await handler(renderMsg({ seq: 2, format: "text" }));

    expect(loadWasmModule).toHaveBeenCalledTimes(1);
    expect(initialize).toHaveBeenCalledTimes(1);
    expect(render).toHaveBeenCalledTimes(2);
    expect(responses).toEqual([
      {
        version: PROTOCOL_VERSION,
        type: "result",
        seq: 1,
        format: "svg",
        output: "svg:graph TD\nA-->B:{}",
      },
      {
        version: PROTOCOL_VERSION,
        type: "result",
        seq: 2,
        format: "text",
        output: "text:graph TD\nA-->B:{}",
      },
    ]);
  });

  it("returns wasm-render-rejected code on generic render failure", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmExports> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => {
          throw new Error("unknown output format: bad");
        },
        renderWithBrowserTextMetrics: () => "unused",
        validate: () => "unused",
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      postMessage: (m) => responses.push(m),
    });

    await handler(renderMsg({ seq: 7 }));
    expect(responses).toEqual([
      {
        version: PROTOCOL_VERSION,
        type: "error",
        seq: 7,
        error: "unknown output format: bad",
        code: "wasm-render-rejected",
      },
    ]);
  });

  it("returns wasm-config-rejected when wasm reports invalid config_json", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmExports> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => {
          throw new Error("invalid config_json: trailing comma");
        },
        renderWithBrowserTextMetrics: () => "unused",
        validate: () => "unused",
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      postMessage: (m) => responses.push(m),
    });

    await handler(renderMsg({ seq: 8 }));
    expect(responses[0]).toMatchObject({
      type: "error",
      seq: 8,
      code: "wasm-config-rejected",
    });
  });

  it("routes validate to wasm.validate and posts validation response", async () => {
    const validate = vi.fn(
      (input: string) => `{"valid":true,"input":"${input}"}`,
    );
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmExports> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => "unused",
        renderWithBrowserTextMetrics: () => "unused",
        validate,
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      postMessage: (m) => responses.push(m),
    });

    await handler({
      version: PROTOCOL_VERSION,
      type: "validate",
      seq: 3,
      input: "graph TD\nA-->B",
    });
    expect(validate).toHaveBeenCalledWith("graph TD\nA-->B");
    expect(responses).toEqual([
      {
        version: PROTOCOL_VERSION,
        type: "validation",
        seq: 3,
        resultJson: '{"valid":true,"input":"graph TD\nA-->B"}',
      },
    ]);
  });

  it("routes resolveBrowserTextMetrics to wasm.browserTextMetricsRequest without preparing", async () => {
    const browserTextMetricsRequest = vi.fn(
      () =>
        '{"required":true,"browserTextMetrics":{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"lineHeight":24,"cssFont":"16px Inter"}]}}',
    );
    const prepareBrowserTextMetrics = vi.fn();
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmExports> => ({
        default: async () => {},
        browserTextMetricsRequest,
        render: () => "unused",
        renderWithBrowserTextMetrics: () => "unused",
        validate: () => "unused",
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      prepareBrowserTextMetrics,
      postMessage: (m) => responses.push(m),
    });

    await handler({
      version: PROTOCOL_VERSION,
      type: "resolveBrowserTextMetrics",
      seq: 4,
      input: "graph TD\nA",
      format: "svg",
      configJson: "{}",
    });

    expect(prepareBrowserTextMetrics).not.toHaveBeenCalled();
    expect(responses[0]?.type).toBe("browserTextMetricsDecision");
    if (responses[0]?.type === "browserTextMetricsDecision") {
      expect(responses[0].seq).toBe(4);
      expect(responses[0].decision.required).toBe(true);
    }
  });

  it("rejects malformed decision JSON from wasm with an error response", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmExports> => ({
        default: async () => {},
        browserTextMetricsRequest: () => '{"required":"not-a-boolean"}',
        render: () => "unused",
        renderWithBrowserTextMetrics: () => "unused",
        validate: () => "unused",
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      postMessage: (m) => responses.push(m),
    });

    await handler({
      version: PROTOCOL_VERSION,
      type: "resolveBrowserTextMetrics",
      seq: 99,
      input: "graph TD\nA",
      format: "svg",
      configJson: "{}",
    });

    expect(responses[0]?.type).toBe("error");
    if (responses[0]?.type === "error") {
      expect(responses[0].seq).toBe(99);
      expect(responses[0].error).toMatch(/malformed decision payload/);
    }
  });

  it("renderWithBrowserTextMetrics — prepares then calls wasm dynamic export", async () => {
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
        `${format}:${input}:${configJson}:${metricsJson}:${callback("A", "f")}`,
    );
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmExports> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => "static unused",
        renderWithBrowserTextMetrics,
        validate: () => "unused",
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      prepareBrowserTextMetrics,
      postMessage: (m) => responses.push(m),
    });

    await handler(dynamicMsg({ seq: 9 }));

    expect(prepareBrowserTextMetrics).toHaveBeenCalledWith({
      request: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
      environment: undefined,
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
        version: PROTOCOL_VERSION,
        type: "result",
        seq: 9,
        format: "svg",
        output: 'svg:graph TD\nA-->B:{}:{"cssFont":"16px Inter"}:42',
      },
    ]);
  });

  it("plain prepare failure → structured error without bridge code", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmExports> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => "unused",
        renderWithBrowserTextMetrics: () => "unused",
        validate: () => "unused",
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      prepareBrowserTextMetrics: vi.fn(async () => {
        throw new Error("Dynamic text metrics require OffscreenCanvas");
      }),
      postMessage: (m) => responses.push(m),
    });

    await handler(dynamicMsg({ seq: 10 }));
    expect(responses).toEqual([
      {
        version: PROTOCOL_VERSION,
        type: "error",
        seq: 10,
        error: "Dynamic text metrics require OffscreenCanvas",
      },
    ]);
  });

  it("Q6 §3.8a — fallbackEligible: true capability error masked to dynamic-metrics-capability", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmExports> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => "unused",
        renderWithBrowserTextMetrics: () => "unused",
        validate: () => "unused",
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      prepareBrowserTextMetrics: vi.fn(async () => {
        throw new MmdsBrowserTextMetricsCapabilityError({
          code: "worker-offscreen-canvas-unavailable",
          message:
            "Dynamic text metrics require OffscreenCanvas in the worker.",
        });
      }),
      postMessage: (m) => responses.push(m),
    });

    await handler(dynamicMsg({ seq: 11 }));

    expect(responses).toEqual([
      {
        version: PROTOCOL_VERSION,
        type: "error",
        seq: 11,
        error: "Dynamic text metrics require OffscreenCanvas in the worker.",
        code: "dynamic-metrics-capability",
      },
    ]);
    expect(JSON.stringify(responses[0])).not.toContain(
      "worker-offscreen-canvas-unavailable",
    );
  });

  it("Q6 §3.8b — fallbackEligible: false capability error does NOT get bridge code", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmExports> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => "unused",
        renderWithBrowserTextMetrics: () => "unused",
        validate: () => "unused",
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      prepareBrowserTextMetrics: vi.fn(async () => {
        throw new MmdsBrowserTextMetricsCapabilityError({
          code: "main-thread-font-face-set-unavailable",
          message:
            "Dynamic text metrics require document.fonts on the main thread.",
        });
      }),
      postMessage: (m) => responses.push(m),
    });

    await handler(dynamicMsg({ seq: 12 }));

    expect(responses).toEqual([
      {
        version: PROTOCOL_VERSION,
        type: "error",
        seq: 12,
        error:
          "Dynamic text metrics require document.fonts on the main thread.",
      },
    ]);
    expect(responses[0]).not.toHaveProperty("code");
  });

  it("unsupported protocol version rejected with unsupported-format", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmExports> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => "unused",
        renderWithBrowserTextMetrics: () => "unused",
        validate: () => "unused",
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      postMessage: (m) => responses.push(m),
    });

    // biome-ignore lint/suspicious/noExplicitAny: test exercises invalid wire payload
    await handler({ version: 2, type: "render", seq: 15 } as any);

    expect(loadWasmModule).not.toHaveBeenCalled();
    expect(responses).toEqual([
      {
        version: PROTOCOL_VERSION,
        type: "error",
        seq: 15,
        error: "Unsupported worker-protocol version: 2",
        code: "unsupported-format",
      },
    ]);
  });

  it("classifies wasm 're-entered' rejection as wasm-reentered", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmExports> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => "unused",
        renderWithBrowserTextMetrics: () => {
          throw new Error("renderWithBrowserTextMetrics re-entered");
        },
        validate: () => "unused",
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      prepareBrowserTextMetrics: vi.fn(async () => ({
        metricsJson: "{}",
        measureText: () => 0,
      })),
      postMessage: (m) => responses.push(m),
    });

    await handler(dynamicMsg({ seq: 16 }));
    expect(responses[0]).toMatchObject({
      type: "error",
      seq: 16,
      code: "wasm-reentered",
    });
    if (responses[0]?.type === "error") {
      expect(responses[0].error).toContain("re-entered");
    }
  });

  it("malformed version-1 render payload rejected before wasm loads", async () => {
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmExports> => ({
        default: async () => {},
        browserTextMetricsRequest: () => "unused",
        render: () => "should not be reached",
        renderWithBrowserTextMetrics: () => "unused",
        validate: () => "should not be reached",
      }),
    );
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule,
      postMessage: (m) => responses.push(m),
    });

    // version: 1 but the render envelope is missing input + format + configJson
    await handler({ version: 1, type: "render", seq: 17 });

    expect(loadWasmModule).not.toHaveBeenCalled();
    expect(responses).toEqual([
      {
        version: PROTOCOL_VERSION,
        type: "error",
        seq: 17,
        error: "Malformed worker request payload.",
        code: "unsupported-format",
      },
    ]);
  });

  it("non-object messages and stripped seq still produce a coherent error", async () => {
    const responses: WorkerResponseMessage[] = [];
    const handler = createWorkerRequestHandler({
      loadWasmModule: async () => ({
        default: async () => {},
        browserTextMetricsRequest: () => "",
        render: () => "",
        renderWithBrowserTextMetrics: () => "",
        validate: () => "",
      }),
      postMessage: (m) => responses.push(m),
    });

    await handler("not an object");
    await handler(null);
    await handler(undefined);

    expect(responses).toHaveLength(3);
    for (const response of responses) {
      expect(response).toMatchObject({
        version: PROTOCOL_VERSION,
        type: "error",
        seq: 0,
        code: "unsupported-format",
      });
    }
  });
});
