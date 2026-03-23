import { describe, expect, it, vi } from "vitest";
import { createWorkerRequestHandler } from "./worker";
import type {
  WorkerRequestMessage,
  WorkerResponseMessage,
} from "./worker-protocol";

interface MockWasmModule {
  default: () => Promise<void>;
  render: (input: string, format: string, configJson: string) => string;
  validate: (input: string) => string;
}

describe("createWorkerRequestHandler", () => {
  it("initializes worker once and returns render results", async () => {
    const initialize = vi.fn(async () => {});
    const render = vi.fn(
      (input: string, format: string, configJson: string) => {
        return `${format}:${input}:${configJson}`;
      },
    );
    const validate = vi.fn((input: string) => `{"valid":true,"input":"${input}"}`);
    const loadWasmModule = vi.fn(
      async (): Promise<MockWasmModule> => ({
        default: initialize,
        render,
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
        render: () => {
          throw new Error("unknown output format: bad");
        },
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
        render: () => "unused",
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
});
