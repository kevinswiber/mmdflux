import { describe, expect, it } from "vitest";
import { createRenderWorkerClient } from "./services/render-client";
import type {
  WorkerRequestMessage,
  WorkerResponseMessage,
} from "./worker-protocol";

class MockWorker {
  onmessage: ((event: MessageEvent<WorkerResponseMessage>) => void) | null = null;

  postMessage(message: WorkerRequestMessage): void {
    if (!this.onmessage) {
      throw new Error("worker message handler was not installed");
    }

    queueMicrotask(() => {
      if (message.type === "render") {
        this.onmessage?.({
          data: {
            type: "result",
            seq: message.seq,
            format: message.format,
            output: `${message.format}:${message.input}:${message.configJson}`,
          },
        } as MessageEvent<WorkerResponseMessage>);
        return;
      }

      this.onmessage?.({
        data: {
          type: "validation",
          seq: message.seq,
          resultJson: '{"valid":true}',
        },
      } as MessageEvent<WorkerResponseMessage>);
    });
  }

  terminate(): void {}
}

describe("createRenderWorkerClient", () => {
  it("routes render and validation requests over the same worker", async () => {
    const worker = new MockWorker();
    const client = createRenderWorkerClient(worker as unknown as Worker);

    const renderPromise = client.render({
      seq: 7,
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: '{"padding":2}',
    });
    const validatePromise = client.validate("graph TD\nA-->B");

    await expect(renderPromise).resolves.toEqual({
      seq: 7,
      format: "svg",
      output: 'svg:graph TD\nA-->B:{"padding":2}',
    });
    await expect(validatePromise).resolves.toBe('{"valid":true}');
  });
});
