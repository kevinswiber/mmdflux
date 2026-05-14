import { afterEach, describe, expect, it, vi } from "vitest";
import { MmdsBrowserTextMetricsCapabilityError } from "../src/capability.js";
import {
  type AutoClientOptions,
  createAutoMmdsBrowserTextMetricsClient,
  createDefaultMmdsWorker,
  createMmdsBrowserTextMetricsClient,
} from "../src/client.js";
import type { MmdsMainThreadRenderer } from "../src/main-thread.js";
import {
  PROTOCOL_VERSION,
  type WorkerRequestMessage,
  type WorkerResponseMessage,
} from "../src/worker-protocol.js";

interface MockWorkerOptions {
  suppressDynamicResponse?: boolean;
  dynamicResponse?: WorkerResponseMessage;
  resolverResponse?: WorkerResponseMessage;
  throwOnPost?: boolean;
}

class MockWorker {
  onmessage: ((event: MessageEvent<WorkerResponseMessage>) => void) | null =
    null;
  posts: WorkerRequestMessage[] = [];
  terminated = false;

  constructor(private readonly opts: MockWorkerOptions = {}) {}

  postMessage(message: WorkerRequestMessage): void {
    if (this.opts.throwOnPost) {
      throw new Error("post failed");
    }
    this.posts.push(message);
    if (!this.onmessage) {
      throw new Error("worker message handler was not installed");
    }
    queueMicrotask(() => {
      if (message.type === "render") {
        this.onmessage?.({
          data: {
            version: PROTOCOL_VERSION,
            type: "result",
            seq: message.seq,
            format: message.format,
            output: `${message.format}:${message.input}:${message.configJson}`,
          },
        } as MessageEvent<WorkerResponseMessage>);
        return;
      }
      if (message.type === "validate") {
        this.onmessage?.({
          data: {
            version: PROTOCOL_VERSION,
            type: "validation",
            seq: message.seq,
            resultJson: `{"valid":true,"input":"${message.input}"}`,
          },
        } as MessageEvent<WorkerResponseMessage>);
        return;
      }
      if (message.type === "resolveBrowserTextMetrics") {
        if (this.opts.resolverResponse) {
          this.onmessage?.({
            data: this.opts.resolverResponse,
          } as MessageEvent<WorkerResponseMessage>);
          return;
        }
        const required = message.input.includes("font-family");
        this.onmessage?.({
          data: {
            version: PROTOCOL_VERSION,
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
                        fontSize: 12,
                        lineHeight: 18,
                        cssFont: "12px Verdana",
                      },
                    ],
                  }
                : undefined,
            },
          },
        } as MessageEvent<WorkerResponseMessage>);
        return;
      }
      // renderWithBrowserTextMetrics
      if (this.opts.suppressDynamicResponse) return;
      if (this.opts.dynamicResponse) {
        this.onmessage?.({
          data: this.opts.dynamicResponse,
        } as MessageEvent<WorkerResponseMessage>);
        return;
      }
      this.onmessage?.({
        data: {
          version: PROTOCOL_VERSION,
          type: "result",
          seq: message.seq,
          format: "svg",
          output: `dynamic:${message.input}:${message.browserTextMetrics.fontFamily}`,
        },
      } as MessageEvent<WorkerResponseMessage>);
    });
  }

  terminate(): void {
    this.terminated = true;
  }
}

function fallbackFixture(
  output = "main-thread-output",
): MmdsMainThreadRenderer {
  return {
    renderSvg: vi.fn(async () => ({
      output,
      format: "svg" as const,
      source: "main-thread" as const,
    })),
    renderStatic: vi.fn(async () => ({
      output,
      format: "svg" as const,
      source: "static" as const,
    })),
    validate: vi.fn(async () => '{"valid":true}'),
  };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("createMmdsBrowserTextMetricsClient", () => {
  it("Q6 §3.9a — renderStatic routes through worker; result carries source: worker", async () => {
    const worker = new MockWorker();
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
    });
    const result = await client.renderStatic({
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: '{"padding":2}',
    });
    expect(result).toEqual({
      output: 'svg:graph TD\nA-->B:{"padding":2}',
      format: "svg",
      source: "worker",
    });
    expect(worker.posts[0]).toMatchObject({
      version: PROTOCOL_VERSION,
      type: "render",
      format: "svg",
    });
  });

  it("Q6 §3.9b — resolver and validate run independently and are routed by seq", async () => {
    const worker = new MockWorker();
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
    });
    const [validation, decision] = await Promise.all([
      client.validate("graph TD\nA-->B"),
      client.resolveBrowserTextMetricsRequest({
        input: "graph TD\nA[X]\nstyle A font-family:Verdana",
        format: "svg",
        configJson: "{}",
      }),
    ]);
    expect(validation).toContain('"valid":true');
    expect(decision.required).toBe(true);
    expect(decision.browserTextMetrics?.defaultStyle).toBe("s0");
  });

  it("resolver errors propagate to the caller", async () => {
    const worker = new MockWorker();
    worker.postMessage = ((message: WorkerRequestMessage) => {
      worker.posts.push(message);
      queueMicrotask(() => {
        worker.onmessage?.({
          data: {
            version: PROTOCOL_VERSION,
            type: "error",
            seq: message.seq,
            error: "resolver blew up",
          },
        } as MessageEvent<WorkerResponseMessage>);
      });
    }) as typeof worker.postMessage;
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
    });
    await expect(
      client.resolveBrowserTextMetricsRequest({
        input: "x",
        format: "svg",
        configJson: "{}",
      }),
    ).rejects.toThrow("resolver blew up");
  });

  it("Q6 §3.9c — dynamic-metrics-capability code triggers fallback.renderSvg once", async () => {
    const worker = new MockWorker();
    worker.postMessage = ((message: WorkerRequestMessage) => {
      worker.posts.push(message);
      queueMicrotask(() => {
        worker.onmessage?.({
          data: {
            version: PROTOCOL_VERSION,
            type: "error",
            seq: message.seq,
            error: "no canvas",
            code: "dynamic-metrics-capability",
          },
        } as MessageEvent<WorkerResponseMessage>);
      });
    }) as typeof worker.postMessage;
    const fallback = fallbackFixture("fb-svg");
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
      fallback,
    });
    const result = await client.renderSvg({
      input: "g",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    });
    expect(result).toEqual({
      output: "fb-svg",
      format: "svg",
      source: "main-thread",
    });
    expect(fallback.renderSvg).toHaveBeenCalledTimes(1);
  });

  it("worker error with wasm-* code reconstructs a typed capability error", async () => {
    const worker = new MockWorker();
    worker.postMessage = ((message: WorkerRequestMessage) => {
      worker.posts.push(message);
      queueMicrotask(() => {
        worker.onmessage?.({
          data: {
            version: PROTOCOL_VERSION,
            type: "error",
            seq: message.seq,
            error: "renderWithBrowserTextMetrics re-entered",
            code: "wasm-reentered",
          },
        } as MessageEvent<WorkerResponseMessage>);
      });
    }) as typeof worker.postMessage;
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
    });
    const promise = client.renderStatic({
      input: "g",
      format: "svg",
      configJson: "{}",
    });
    await expect(promise).rejects.toBeInstanceOf(
      MmdsBrowserTextMetricsCapabilityError,
    );
    await expect(promise).rejects.toMatchObject({
      code: "wasm-reentered",
      fallbackEligible: false,
    });
  });

  it("worker error with unknown code surfaces as plain Error (not capability)", async () => {
    const worker = new MockWorker();
    worker.postMessage = ((message: WorkerRequestMessage) => {
      worker.posts.push(message);
      queueMicrotask(() => {
        worker.onmessage?.({
          data: {
            version: PROTOCOL_VERSION,
            type: "error",
            seq: message.seq,
            error: "novel failure",
          },
        } as MessageEvent<WorkerResponseMessage>);
      });
    }) as typeof worker.postMessage;
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
    });
    const promise = client.renderStatic({
      input: "g",
      format: "svg",
      configJson: "{}",
    });
    await expect(promise).rejects.toBeInstanceOf(Error);
    await expect(promise).rejects.not.toBeInstanceOf(
      MmdsBrowserTextMetricsCapabilityError,
    );
    await expect(promise).rejects.toThrow("novel failure");
  });

  it("Q6 §3.9d — timeout triggers fallback when configured and fallback exists", async () => {
    vi.useFakeTimers();
    const worker = new MockWorker({ suppressDynamicResponse: true });
    const fallback = fallbackFixture("fb-timeout");
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
      fallback,
      dynamicMetricsWorkerTimeoutMs: 1000,
    });
    const promise = client.renderSvg({
      input: "g",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    });
    await vi.advanceTimersByTimeAsync(1500);
    const result = await promise;
    expect(result.source).toBe("main-thread");
    expect(result.output).toBe("fb-timeout");
    expect(fallback.renderSvg).toHaveBeenCalledTimes(1);
  });

  it("timeout rejects with a plain Error when no fallback is configured", async () => {
    vi.useFakeTimers();
    const worker = new MockWorker({ suppressDynamicResponse: true });
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
      dynamicMetricsWorkerTimeoutMs: 750,
    });
    const promise = client.renderSvg({
      input: "g",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    });
    // Attach a catch handler before advancing timers so the rejection
    // settled by the fake timer is not flagged as unhandled by vitest.
    const caught = promise.catch((error) => error);
    // The promise must not be left pending forever — the timeout has to
    // fire even when there is no main-thread fallback to escape to.
    await vi.advanceTimersByTimeAsync(1000);
    const error = await caught;
    expect(error).toBeInstanceOf(Error);
    expect(error).not.toBeInstanceOf(MmdsBrowserTextMetricsCapabilityError);
    expect((error as Error).message).toMatch(/timed out after 750ms/);
  });

  it("non-capability worker error does NOT trigger fallback", async () => {
    const worker = new MockWorker();
    worker.postMessage = ((message: WorkerRequestMessage) => {
      worker.posts.push(message);
      queueMicrotask(() => {
        worker.onmessage?.({
          data: {
            version: PROTOCOL_VERSION,
            type: "error",
            seq: message.seq,
            error: "unrelated",
          },
        } as MessageEvent<WorkerResponseMessage>);
      });
    }) as typeof worker.postMessage;
    const fallback = fallbackFixture();
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
      fallback,
    });
    await expect(
      client.renderSvg({
        input: "g",
        browserTextMetrics: {
          fontFamily: "Inter",
          fontSizePx: 16,
          lineHeightPx: 24,
        },
      }),
    ).rejects.toThrow("unrelated");
    expect(fallback.renderSvg).not.toHaveBeenCalled();
  });

  it("post-fallback failure does not retry", async () => {
    const worker = new MockWorker();
    worker.postMessage = ((message: WorkerRequestMessage) => {
      worker.posts.push(message);
      queueMicrotask(() => {
        worker.onmessage?.({
          data: {
            version: PROTOCOL_VERSION,
            type: "error",
            seq: message.seq,
            error: "needs fallback",
            code: "dynamic-metrics-capability",
          },
        } as MessageEvent<WorkerResponseMessage>);
      });
    }) as typeof worker.postMessage;
    const fallback: MmdsMainThreadRenderer = {
      renderSvg: vi.fn(async () => {
        throw new Error("fallback also failed");
      }),
      renderStatic: vi.fn(),
      validate: vi.fn(),
    };
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
      fallback,
    });
    await expect(
      client.renderSvg({
        input: "g",
        browserTextMetrics: {
          fontFamily: "Inter",
          fontSizePx: 16,
          lineHeightPx: 24,
        },
      }),
    ).rejects.toThrow("fallback also failed");
    expect(fallback.renderSvg).toHaveBeenCalledTimes(1);
  });

  it("Q6 §3.10a — validate never invokes fallback", async () => {
    const worker = new MockWorker();
    const fallback = fallbackFixture();
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
      fallback,
    });
    await client.validate("x");
    expect(fallback.renderSvg).not.toHaveBeenCalled();
    expect(fallback.validate).not.toHaveBeenCalled();
  });

  it("Q6 §3.10b — resolver never invokes fallback (errors propagate)", async () => {
    const worker = new MockWorker();
    worker.postMessage = ((message: WorkerRequestMessage) => {
      worker.posts.push(message);
      queueMicrotask(() => {
        if (message.type === "resolveBrowserTextMetrics") {
          worker.onmessage?.({
            data: {
              version: PROTOCOL_VERSION,
              type: "error",
              seq: message.seq,
              error: "resolver fail",
              code: "dynamic-metrics-capability",
            },
          } as MessageEvent<WorkerResponseMessage>);
        }
      });
    }) as typeof worker.postMessage;
    const fallback = fallbackFixture();
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
      fallback,
    });
    await expect(
      client.resolveBrowserTextMetricsRequest({
        input: "x",
        format: "svg",
        configJson: "{}",
      }),
    ).rejects.toThrow("resolver fail");
    expect(fallback.renderSvg).not.toHaveBeenCalled();
  });

  it("terminate() rejects pending promises with a plain Error", async () => {
    const worker = new MockWorker({ suppressDynamicResponse: true });
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
      dynamicMetricsWorkerTimeoutMs: 0, // no timeout
    });
    const promise = client.renderSvg({
      input: "g",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    });
    await new Promise((r) => setImmediate(r));
    client.terminate();
    await expect(promise).rejects.toThrow("render worker terminated");
    await expect(promise).rejects.toBeInstanceOf(Error);
    expect(worker.terminated).toBe(true);
  });

  it("renderAuto skips resolver when heuristic returns false (renders static)", async () => {
    const worker = new MockWorker();
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
    });
    const result = await client.renderAuto({
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: "{}",
    });
    expect(result.source).toBe("worker");
    // No resolver message posted
    expect(
      worker.posts.find((m) => m.type === "resolveBrowserTextMetrics"),
    ).toBeUndefined();
  });

  it("renderAuto invokes resolver then renderSvg when heuristic detects font directive", async () => {
    const worker = new MockWorker();
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
    });
    const result = await client.renderAuto({
      input: "graph TD\nA[X]\nstyle A font-family:Verdana",
      format: "svg",
      configJson: "{}",
    });
    expect(result.source).toBe("worker");
    expect(result.output).toContain("dynamic:");
    expect(worker.posts.map((m) => m.type)).toEqual([
      "resolveBrowserTextMetrics",
      "renderWithBrowserTextMetrics",
    ]);
  });

  it("renderAuto rejects when resolver says required=true but omits browserTextMetrics", async () => {
    const worker = new MockWorker();
    worker.postMessage = ((message: WorkerRequestMessage) => {
      worker.posts.push(message);
      queueMicrotask(() => {
        if (message.type === "resolveBrowserTextMetrics") {
          worker.onmessage?.({
            data: {
              version: PROTOCOL_VERSION,
              type: "browserTextMetricsDecision",
              seq: message.seq,
              decision: { required: true }, // intentionally omits browserTextMetrics
            },
          } as MessageEvent<WorkerResponseMessage>);
        }
      });
    }) as typeof worker.postMessage;
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
    });
    const promise = client.renderAuto({
      input: "graph TD\nA[X]\nstyle A font-family:Verdana",
      format: "svg",
      configJson: "{}",
    });
    await expect(promise).rejects.toBeInstanceOf(
      MmdsBrowserTextMetricsCapabilityError,
    );
    await expect(promise).rejects.toMatchObject({
      code: "invalid-text-metrics-request",
      fallbackEligible: false,
    });
    // No renderWithBrowserTextMetrics nor static render was attempted —
    // the inconsistent resolver response short-circuits without silent fallback.
    expect(worker.posts.map((m) => m.type)).toEqual([
      "resolveBrowserTextMetrics",
    ]);
  });

  it("renderAuto falls back to renderStatic when resolver says required=false", async () => {
    // Heuristic triggers (font-family in input) but resolver overrides to required=false
    const worker = new MockWorker({
      resolverResponse: {
        version: PROTOCOL_VERSION,
        type: "browserTextMetricsDecision",
        seq: 0, // overridden in the inline handler below
        decision: { required: false },
      },
    });
    worker.postMessage = ((message: WorkerRequestMessage) => {
      worker.posts.push(message);
      queueMicrotask(() => {
        if (message.type === "resolveBrowserTextMetrics") {
          worker.onmessage?.({
            data: {
              version: PROTOCOL_VERSION,
              type: "browserTextMetricsDecision",
              seq: message.seq,
              decision: { required: false },
            },
          } as MessageEvent<WorkerResponseMessage>);
          return;
        }
        if (message.type === "render") {
          worker.onmessage?.({
            data: {
              version: PROTOCOL_VERSION,
              type: "result",
              seq: message.seq,
              format: message.format,
              output: `static-out:${message.input}`,
            },
          } as MessageEvent<WorkerResponseMessage>);
        }
      });
    }) as typeof worker.postMessage;
    const client = createMmdsBrowserTextMetricsClient({
      worker: worker,
    });
    const result = await client.renderAuto({
      input: "graph TD\nA[X]\nstyle A font-family:Verdana",
      format: "svg",
      configJson: "{}",
    });
    expect(result.source).toBe("worker");
    expect(result.output).toContain("static-out");
    expect(worker.posts.map((m) => m.type)).toEqual([
      "resolveBrowserTextMetrics",
      "render",
    ]);
  });
});

describe("createDefaultMmdsWorker", () => {
  it("constructs a Worker via the bundler-coupled URL", () => {
    const created: unknown[] = [];
    class FakeWorker {
      constructor(url: URL | string, options: WorkerOptions | undefined) {
        created.push({ url: url.toString(), options });
      }
      postMessage() {}
      terminate() {}
      addEventListener() {}
      removeEventListener() {}
      dispatchEvent() {
        return true;
      }
      onmessage = null;
      onmessageerror = null;
      onerror = null;
    }
    vi.stubGlobal("Worker", FakeWorker);
    try {
      const worker = createDefaultMmdsWorker();
      expect(worker).toBeInstanceOf(FakeWorker);
      expect(created).toHaveLength(1);
      const entry = created[0] as { url: string; options: WorkerOptions };
      expect(entry.url).toContain("worker.js");
      expect(entry.options).toMatchObject({ type: "module" });
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("createAutoMmdsBrowserTextMetricsClient", () => {
  function setWorkerDefined(defined: boolean) {
    if (defined) {
      vi.stubGlobal(
        "Worker",
        class FakeWorker {
          postMessage() {}
          terminate() {}
          addEventListener() {}
          removeEventListener() {}
          dispatchEvent() {
            return true;
          }
          onmessage = null;
          onmessageerror = null;
          onerror = null;
        },
      );
    } else {
      vi.stubGlobal("Worker", undefined);
    }
  }

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("Worker defined + workerFactory + mainThreadFactory → client with worker and fallback", () => {
    setWorkerDefined(true);
    const opts: AutoClientOptions = {
      workerFactory: () => new MockWorker(),
      mainThreadFactory: () => fallbackFixture(),
    };
    const result = createAutoMmdsBrowserTextMetricsClient(opts);
    expect(result).toHaveProperty("renderSvg");
    expect(result).toHaveProperty("renderStatic");
    expect(result).toHaveProperty("terminate");
  });

  it("Worker undefined + only mainThreadFactory → main-thread renderer", () => {
    setWorkerDefined(false);
    const renderer = fallbackFixture();
    const result = createAutoMmdsBrowserTextMetricsClient({
      mainThreadFactory: () => renderer,
    });
    expect(result).toBe(renderer);
  });

  it("mainThreadFactory-only (Worker defined) still returns the main-thread renderer", () => {
    setWorkerDefined(true);
    const renderer = fallbackFixture();
    const result = createAutoMmdsBrowserTextMetricsClient({
      mainThreadFactory: () => renderer,
    });
    expect(result).toBe(renderer);
  });

  it("workerFactory-only + Worker defined → worker-backed client", () => {
    setWorkerDefined(true);
    const result = createAutoMmdsBrowserTextMetricsClient({
      workerFactory: () => new MockWorker(),
    });
    expect(result).toHaveProperty("terminate");
    expect(result).toHaveProperty("renderSvg");
  });

  it("workerFactory-only + Worker undefined → throws unsupported-format", () => {
    setWorkerDefined(false);
    expect(() =>
      createAutoMmdsBrowserTextMetricsClient({
        workerFactory: () => new MockWorker(),
      }),
    ).toThrow(MmdsBrowserTextMetricsCapabilityError);
    try {
      createAutoMmdsBrowserTextMetricsClient({
        workerFactory: () => new MockWorker(),
      });
    } catch (err) {
      expect(err).toMatchObject({
        code: "unsupported-format",
        fallbackEligible: false,
      });
    }
  });

  it("neither factory provided → throws unsupported-format", () => {
    setWorkerDefined(true);
    expect(() => createAutoMmdsBrowserTextMetricsClient({})).toThrow(
      MmdsBrowserTextMetricsCapabilityError,
    );
  });
});
