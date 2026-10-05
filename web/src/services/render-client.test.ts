import { describe, expect, it, vi } from "vitest";
import { createRenderWorkerClient } from "./render-client";

function fakeWorker() {
  const worker = {
    onmessage: null as ((event: MessageEvent) => void) | null,
    postMessage: vi.fn(),
    terminate: vi.fn(),
  };
  const respond = (data: unknown): void => {
    worker.onmessage?.({ data } as MessageEvent);
  };
  return { worker, respond };
}

describe("renderFitted", () => {
  it("posts a fitted render request and resolves with the output and fit report", async () => {
    const { worker, respond } = fakeWorker();
    const client = createRenderWorkerClient(worker as unknown as Worker);

    const pending = client.renderFitted({
      seq: 4,
      input: "graph LR\nA-->B",
      format: "text",
      configJson: "{}",
      fitJson: '{"maxWidth":40}',
    });

    expect(worker.postMessage).toHaveBeenCalledWith({
      version: 1,
      type: "renderFitted",
      seq: 4,
      input: "graph LR\nA-->B",
      format: "text",
      configJson: "{}",
      fitJson: '{"maxWidth":40}',
    });

    const fit = {
      outcome: "asAuthored",
      size: { width: 12, height: 5 },
      asAuthored: { width: 12, height: 5 },
      applied: [],
    };
    respond({
      version: 1,
      type: "fittedResult",
      seq: 4,
      format: "text",
      output: "A --> B",
      fit,
    });

    await expect(pending).resolves.toEqual({
      seq: 4,
      format: "text",
      output: "A --> B",
      fit,
    });
  });

  it("rejects with the worker error for a fitted request", async () => {
    const { worker, respond } = fakeWorker();
    const client = createRenderWorkerClient(worker as unknown as Worker);

    const pending = client.renderFitted({
      seq: 5,
      input: "graph LR\nA-->B",
      format: "text",
      configJson: "{}",
      fitJson: '{"maxWidth":0}',
    });
    respond({
      version: 1,
      type: "error",
      seq: 5,
      error: "max width must be at least 1",
    });

    await expect(pending).rejects.toThrow("max width must be at least 1");
  });
});
