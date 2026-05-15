import { vi } from "vitest";
import type { MainThreadBrowserTextMetricsRenderer } from "../services/main-thread-browser-text-metrics";
import {
  PROTOCOL_VERSION,
  type WorkerRequestMessage,
  type WorkerResponseMessage,
} from "../worker-protocol";

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

      if (message.type === "renderWithBrowserTextMetrics") {
        if (this.options.suppressDynamicResponse) {
          return;
        }

        if (this.options.dynamicResponse) {
          this.onmessage?.({
            data: this.options.dynamicResponse,
          } as MessageEvent<WorkerResponseMessage>);
          return;
        }

        this.onmessage?.({
          data: {
            version: PROTOCOL_VERSION,
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
            data: this.options.resolverResponse,
          } as MessageEvent<WorkerResponseMessage>);
          return;
        }

        this.onmessage?.({
          data: {
            version: PROTOCOL_VERSION,
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
          version: PROTOCOL_VERSION,
          type: "validation",
          seq: message.seq,
          resultJson: '{"valid":true}',
        },
      } as MessageEvent<WorkerResponseMessage>);
    });
  }

  terminate(): void {}
}

export function mainThreadRendererFixture(
  output = "main-thread-svg",
): MainThreadBrowserTextMetricsRenderer {
  return {
    renderWithBrowserTextMetrics: vi.fn(async (request) => ({
      seq: request.seq,
      format: "svg" as const,
      output,
    })),
  };
}
