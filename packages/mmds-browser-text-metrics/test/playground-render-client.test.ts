import { describe, expect, it } from "vitest";
import { createMmdsBrowserTextMetricsClient } from "../src/client.js";
import { PROTOCOL_VERSION } from "../src/worker-protocol.js";
import { MockRenderWorker, mainThreadRendererFixture } from "./_fixtures.js";

describe("createMmdsBrowserTextMetricsClient (migrated playground)", () => {
  it("routes render and validation requests over the same worker", async () => {
    const worker = new MockRenderWorker();
    const client = createMmdsBrowserTextMetricsClient({ worker });

    const renderPromise = client.renderStatic({
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: '{"padding":2}',
    });
    const validatePromise = client.validate("graph TD\nA-->B");

    await expect(renderPromise).resolves.toEqual({
      format: "svg",
      output: 'svg:graph TD\nA-->B:{"padding":2}',
      source: "worker",
    });
    await expect(validatePromise).resolves.toBe('{"valid":true}');
  });

  it("resolves browser text metrics decisions and posts a resolveBrowserTextMetrics message", async () => {
    const worker = new MockRenderWorker();
    const client = createMmdsBrowserTextMetricsClient({ worker });

    const response = await client.resolveBrowserTextMetricsRequest({
      input: "graph TD\nA[Regular]\nstyle A font-family:Verdana,font-size:8px",
      format: "svg",
      configJson: "{}",
    });

    expect(response).toEqual({
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
    });
    expect(worker.messages.at(-1)).toMatchObject({
      version: PROTOCOL_VERSION,
      type: "resolveBrowserTextMetrics",
      input: "graph TD\nA[Regular]\nstyle A font-family:Verdana,font-size:8px",
      format: "svg",
      configJson: "{}",
    });
  });

  it("propagates browser text metrics resolver errors", async () => {
    const worker = new MockRenderWorker({
      resolverResponse: {
        version: PROTOCOL_VERSION,
        type: "error",
        seq: 0,
        error: "RenderConfig.graph_text_style is not supported",
      },
    });
    const client = createMmdsBrowserTextMetricsClient({ worker });

    await expect(
      client.resolveBrowserTextMetricsRequest({
        input: "graph TD\nA-->B",
        format: "svg",
        configJson: '{"fontFamily":"Inter","fontSize":16}',
      }),
    ).rejects.toThrow("graph_text_style");
  });

  it("keeps resolver, render, and validation responses independent", async () => {
    const worker = new MockRenderWorker();
    const client = createMmdsBrowserTextMetricsClient({ worker });

    const resolverPromise = client.resolveBrowserTextMetricsRequest({
      input: "graph TD\nA[Regular]\nstyle A font-family:Verdana,font-size:8px",
      format: "svg",
      configJson: "{}",
    });
    const renderPromise = client.renderStatic({
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: "{}",
    });
    const validatePromise = client.validate("graph TD\nA-->B");

    await expect(resolverPromise).resolves.toMatchObject({ required: true });
    await expect(renderPromise).resolves.toEqual({
      format: "svg",
      output: "svg:graph TD\nA-->B:{}",
      source: "worker",
    });
    await expect(validatePromise).resolves.toBe('{"valid":true}');
  });

  it("posts dynamic browser text metrics render requests separately", async () => {
    const worker = new MockRenderWorker();
    const fallback = mainThreadRendererFixture();
    const client = createMmdsBrowserTextMetricsClient({ worker, fallback });

    const response = await client.renderSvg({
      input: "graph TD\nA-->B",
      configJson: "{}",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    });

    expect(fallback.renderSvg).not.toHaveBeenCalled();
    expect(response).toEqual({
      format: "svg",
      output: "svg:graph TD\nA-->B:{}:Inter",
      source: "worker",
    });
    expect(worker.messages.at(-1)).toMatchObject({
      version: PROTOCOL_VERSION,
      type: "renderWithBrowserTextMetrics",
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: "{}",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    });
  });

  it("falls back to main-thread dynamic rendering on worker capability errors", async () => {
    const worker = new MockRenderWorker({
      dynamicResponse: {
        version: PROTOCOL_VERSION,
        type: "error",
        seq: 0,
        error: "Dynamic text metrics require OffscreenCanvas in the worker.",
        code: "dynamic-metrics-capability",
      },
    });
    const fallback = mainThreadRendererFixture();
    const client = createMmdsBrowserTextMetricsClient({ worker, fallback });

    await expect(
      client.renderSvg({
        input: "graph TD\nA-->B",
        configJson: "{}",
        browserTextMetrics: {
          fontFamily: "Inter",
          fontSizePx: 16,
          lineHeightPx: 24,
        },
      }),
    ).resolves.toMatchObject({
      format: "svg",
      source: "main-thread",
    });
    expect(fallback.renderSvg).toHaveBeenCalledTimes(1);
  });

  it("falls back to main-thread dynamic rendering when the worker does not respond", async () => {
    const worker = new MockRenderWorker({ suppressDynamicResponse: true });
    const fallback = mainThreadRendererFixture();
    const client = createMmdsBrowserTextMetricsClient({
      worker,
      fallback,
      dynamicMetricsWorkerTimeoutMs: 1,
    });

    await expect(
      client.renderSvg({
        input: "graph TD\nA-->B",
        configJson: "{}",
        browserTextMetrics: {
          fontFamily: "Arial",
          fontSizePx: 16,
          lineHeightPx: 24,
        },
      }),
    ).resolves.toMatchObject({
      format: "svg",
      source: "main-thread",
    });
    expect(fallback.renderSvg).toHaveBeenCalledTimes(1);
  });

  it("does not fallback on ordinary dynamic worker errors", async () => {
    const worker = new MockRenderWorker({
      dynamicResponse: {
        version: PROTOCOL_VERSION,
        type: "error",
        seq: 0,
        error: "Dynamic text metrics unavailable for font Inter.",
      },
    });
    const fallback = mainThreadRendererFixture();
    const client = createMmdsBrowserTextMetricsClient({ worker, fallback });

    await expect(
      client.renderSvg({
        input: "graph TD\nA-->B",
        configJson: "{}",
        browserTextMetrics: {
          fontFamily: "Inter",
          fontSizePx: 16,
          lineHeightPx: 24,
        },
      }),
    ).rejects.toThrow("unavailable");
    expect(fallback.renderSvg).not.toHaveBeenCalled();
  });

  it("does not fallback when posting dynamic requests fails", async () => {
    const worker = new MockRenderWorker({ throwOnDynamicPost: true });
    const fallback = mainThreadRendererFixture();
    const client = createMmdsBrowserTextMetricsClient({ worker, fallback });

    await expect(
      client.renderSvg({
        input: "graph TD\nA-->B",
        configJson: "{}",
        browserTextMetrics: {
          fontFamily: "Inter",
          fontSizePx: 16,
          lineHeightPx: 24,
        },
      }),
    ).rejects.toThrow(/post|render/i);
    expect(fallback.renderSvg).not.toHaveBeenCalled();
  });

  it("does not use main-thread dynamic rendering for validation", async () => {
    const worker = new MockRenderWorker();
    const fallback = mainThreadRendererFixture();
    const client = createMmdsBrowserTextMetricsClient({ worker, fallback });

    await expect(client.validate("graph TD\nA-->B")).resolves.toBe(
      '{"valid":true}',
    );
    expect(fallback.renderSvg).not.toHaveBeenCalled();
  });
});
