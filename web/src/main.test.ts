import { readFile } from "node:fs/promises";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import type { BrowserTextMetricsRequest } from "./browser-text-metrics";
import { createDefaultRenderWorkerClient, renderApp } from "./main";
import type { BrowserTextMetricsRenderRequest } from "./services/render-client";

function multiStyleBrowserMetricsRequest(): BrowserTextMetricsRequest {
  return {
    defaultStyle: "s0",
    textStyles: [
      {
        id: "s0",
        fontFamily: "Verdana",
        fontSize: 8,
        lineHeight: 12,
        fontStyle: "normal",
        fontWeight: "400",
        cssFont: "8px Verdana",
      },
      {
        id: "s1",
        fontFamily: "Courier New",
        fontSize: 20,
        lineHeight: 30,
        fontStyle: "normal",
        fontWeight: "400",
        cssFont: "20px Courier New",
      },
      {
        id: "s2",
        fontFamily: "Times New Roman",
        fontSize: 32,
        lineHeight: 48,
        fontStyle: "normal",
        fontWeight: "400",
        cssFont: "32px Times New Roman",
      },
    ],
  };
}

describe("renderApp", () => {
  const fontStyledInput =
    "graph TD\nA[Regular]-->B\nstyle A font-family:Verdana";

  it("main bootstraps the app without owning render or persistence logic", async () => {
    const source = await readFile(
      path.resolve(process.cwd(), "src/main.ts"),
      "utf8",
    );

    expect(source).toMatch(/bootstrapPlaygroundApp/);
    expect(source).not.toMatch(/localStorage\.setItem/);
    expect(source).not.toMatch(/new Worker/);
  });

  it("wires main-thread browser metrics fallback into the default worker client", () => {
    const client = {
      render: vi.fn(),
      renderWithBrowserTextMetrics: vi.fn(),
      resolveBrowserTextMetricsRequest: vi.fn(),
      validate: vi.fn(),
      terminate: vi.fn(),
    };
    const fallbackRenderer = {
      renderWithBrowserTextMetrics: vi.fn(),
    };
    const createClient = vi.fn(() => client);
    const createFallbackRenderer = vi.fn(() => fallbackRenderer);

    vi.stubGlobal("Worker", class FakeWorker {});
    try {
      expect(
        createDefaultRenderWorkerClient(createClient, createFallbackRenderer),
      ).toBe(client);
    } finally {
      vi.unstubAllGlobals();
    }

    expect(createFallbackRenderer).toHaveBeenCalledTimes(1);
    expect(createClient).toHaveBeenCalledWith(undefined, {
      mainThreadBrowserTextMetricsRenderer: fallbackRenderer,
    });
  });

  it("renders redesigned playground shell", () => {
    try {
      history.replaceState(null, "", window.location.pathname);

      const root = document.createElement("div");
      renderApp(root, {
        renderClientFactory: () => ({
          render: async (request) => ({
            seq: request.seq,
            format: request.format,
            output: `${request.format}:${request.input}`,
          }),
          renderWithBrowserTextMetrics: async (request) => ({
            seq: request.seq,
            format: "svg",
            output: `svg:${request.input}`,
          }),
          resolveBrowserTextMetricsRequest: async () => ({ required: false }),
          validate: async () => '{"valid":true}',
          terminate: () => {},
        }),
        stateStorage: {
          getItem: () => null,
          setItem: () => {},
        },
      });
      const exampleSelect = root.querySelector<HTMLSelectElement>(
        "[data-example-select]",
      );
      const activeFormat = root.querySelector<HTMLButtonElement>(
        ".format-tabs button.is-active",
      );

      expect(root.textContent).toContain("mmdflux playground");
      expect(root.textContent).toContain("Advanced controls");
      expect(root.textContent).toContain("Syntax snippets");
      expect(activeFormat?.dataset.format).toBe("svg");
      expect(root.querySelector("[data-preview-controls]")).not.toBeNull();
      expect(root.querySelector("[data-theme-toggle]")).not.toBeNull();
      expect(exampleSelect?.value).toBe("__draft__");
      expect(window.__mmdfluxDebug).toBeUndefined();
    } finally {
      history.replaceState(null, "", window.location.pathname);
      delete window.__mmdfluxDebug;
    }
  });

  it("routes live svg renders with font styles through browser metrics", async () => {
    try {
      history.replaceState(null, "", window.location.pathname);

      const render = vi.fn(async (request) => ({
        seq: request.seq,
        format: request.format,
        output: `${request.format}:static`,
      }));
      const renderWithBrowserTextMetrics = vi.fn(async (request) => ({
        seq: request.seq,
        format: "svg" as const,
        output: `<svg>${request.browserTextMetrics.textStyles?.length}</svg>`,
      }));
      const resolveBrowserTextMetricsRequest = vi.fn(async () => ({
        required: true,
        browserTextMetrics: {
          defaultStyle: "s0",
          textStyles: [
            {
              id: "s0",
              fontFamily: "Verdana",
              fontSize: 8,
              lineHeight: 12,
              fontStyle: "normal",
              fontWeight: "400",
            },
          ],
        },
      }));

      const root = document.createElement("div");
      renderApp(root, {
        renderClientFactory: () => ({
          render,
          renderWithBrowserTextMetrics,
          resolveBrowserTextMetricsRequest,
          validate: async () => '{"valid":true}',
          terminate: () => {},
        }),
        debounceMs: 0,
        stateStorage: {
          getItem: () =>
            JSON.stringify({
              v: 4,
              input: fontStyledInput,
              format: "svg",
              renderSettings: {},
              textPreviewMode: "plain",
              selectedExampleId: "__draft__",
              customInput: fontStyledInput,
            }),
          setItem: () => {},
        },
      });

      await vi.waitFor(() => {
        expect(renderWithBrowserTextMetrics).toHaveBeenCalledTimes(1);
      });

      expect(resolveBrowserTextMetricsRequest).toHaveBeenCalledWith(
        expect.objectContaining({ format: "svg", input: fontStyledInput }),
      );
      expect(renderWithBrowserTextMetrics).toHaveBeenCalledWith({
        seq: expect.any(Number),
        input: expect.any(String),
        configJson: expect.any(String),
        browserTextMetrics: expect.objectContaining({ defaultStyle: "s0" }),
      });
      expect(render).not.toHaveBeenCalled();
      expect(root.querySelector("[data-preview-output]")?.textContent).toBe(
        "1",
      );
    } finally {
      history.replaceState(null, "", window.location.pathname);
    }
  });

  it("keeps unstyled svg live renders on the static path", async () => {
    try {
      history.replaceState(null, "", window.location.pathname);

      const render = vi.fn(async (request) => ({
        seq: request.seq,
        format: request.format,
        output: `${request.format}:static`,
      }));
      const renderWithBrowserTextMetrics = vi.fn(async (request) => ({
        seq: request.seq,
        format: "svg" as const,
        output: `<svg>${request.browserTextMetrics.textStyles?.length}</svg>`,
      }));
      const resolveBrowserTextMetricsRequest = vi.fn(async () => ({
        required: false,
      }));

      const root = document.createElement("div");
      renderApp(root, {
        renderClientFactory: () => ({
          render,
          renderWithBrowserTextMetrics,
          resolveBrowserTextMetricsRequest,
          validate: async () => '{"valid":true}',
          terminate: () => {},
        }),
        debounceMs: 0,
        stateStorage: {
          getItem: () => null,
          setItem: () => {},
        },
      });

      await vi.waitFor(() => {
        expect(render).toHaveBeenCalledTimes(1);
      });

      expect(resolveBrowserTextMetricsRequest).not.toHaveBeenCalled();
      expect(renderWithBrowserTextMetrics).not.toHaveBeenCalled();
    } finally {
      history.replaceState(null, "", window.location.pathname);
    }
  });

  it("installs a query-gated browser metrics debug console helper", async () => {
    const renderWithBrowserTextMetrics = vi.fn(
      async (request: BrowserTextMetricsRenderRequest) => ({
        seq: request.seq,
        format: "svg" as const,
        output: `<span>${request.browserTextMetrics.fontFamily}:${request.input}</span>`,
      }),
    );
    const mainThreadRenderWithBrowserTextMetrics = vi.fn(
      async (request: BrowserTextMetricsRenderRequest) => ({
        seq: request.seq,
        format: "svg" as const,
        output: `<span>main-thread:${request.input}</span>`,
      }),
    );

    try {
      history.replaceState(null, "", "?debugBrowserMetrics=1");

      const root = document.createElement("div");
      renderApp(root, {
        renderClientFactory: () => ({
          render: async (request) => ({
            seq: request.seq,
            format: request.format,
            output: `${request.format}:${request.input}`,
          }),
          renderWithBrowserTextMetrics,
          resolveBrowserTextMetricsRequest: async () => ({ required: false }),
          validate: async () => '{"valid":true}',
          terminate: () => {},
        }),
        mainThreadBrowserTextMetricsRendererFactory: () => ({
          renderWithBrowserTextMetrics: mainThreadRenderWithBrowserTextMetrics,
        }),
        debounceMs: 10_000,
        stateStorage: {
          getItem: () => null,
          setItem: () => {},
        },
      });

      const debug = window.__mmdfluxDebug;
      expect(debug).toBeDefined();

      const workerResult = await debug?.renderBrowserMetrics({
        input: "graph TD\nA-->B",
        fontFamily: "Arial",
        fontSizePx: 18,
        lineHeightPx: 27,
      });

      expect(workerResult).toMatchObject({
        format: "svg",
        output: "<span>Arial:graph TD\nA-->B</span>",
        source: "worker-client",
      });
      expect(renderWithBrowserTextMetrics).toHaveBeenCalledWith({
        seq: expect.any(Number),
        input: "graph TD\nA-->B",
        configJson: '{"pathSimplification":"lossless"}',
        browserTextMetrics: {
          fontFamily: "Arial",
          fontSizePx: 18,
          lineHeightPx: 27,
        },
      });
      expect(root.querySelector("[data-preview-output]")?.textContent).toBe(
        "Arial:graph TD\nA-->B",
      );

      await debug?.renderBrowserMetricsMainThread({
        input: "graph TD\nM-->N",
        show: false,
      });

      expect(mainThreadRenderWithBrowserTextMetrics).toHaveBeenCalledWith({
        seq: expect.any(Number),
        input: "graph TD\nM-->N",
        configJson: '{"pathSimplification":"lossless"}',
        browserTextMetrics: {
          fontFamily: "Arial, sans-serif",
          fontSizePx: 16,
          lineHeightPx: 24,
        },
      });
      expect(renderWithBrowserTextMetrics).toHaveBeenCalledTimes(1);
    } finally {
      history.replaceState(null, "", window.location.pathname);
      delete window.__mmdfluxDebug;
    }
  });

  it("debug console helper accepts worker and main-thread style-set metrics", async () => {
    const renderWithBrowserTextMetrics = vi.fn(
      async (request: BrowserTextMetricsRenderRequest) => ({
        seq: request.seq,
        format: "svg" as const,
        output: `<span>worker:${request.browserTextMetrics.textStyles?.length}</span>`,
      }),
    );
    const mainThreadRenderWithBrowserTextMetrics = vi.fn(
      async (request: BrowserTextMetricsRenderRequest) => ({
        seq: request.seq,
        format: "svg" as const,
        output: `<span>main-thread:${request.browserTextMetrics.textStyles?.length}</span>`,
      }),
    );
    const multiFontInput = `graph TD
    A[Regular] -->|link| B(Styled Node)
    style A font-family:Verdana,font-size:8px
    style B font-family:Courier New,font-size:20px
    linkStyle 0 font-family:Times New Roman,font-size:32px`;
    const browserTextMetrics = multiStyleBrowserMetricsRequest();

    try {
      history.replaceState(null, "", "?debugBrowserMetrics=1");

      const root = document.createElement("div");
      renderApp(root, {
        renderClientFactory: () => ({
          render: async (request) => ({
            seq: request.seq,
            format: request.format,
            output: `${request.format}:${request.input}`,
          }),
          renderWithBrowserTextMetrics,
          resolveBrowserTextMetricsRequest: async () => ({ required: false }),
          validate: async () => '{"valid":true}',
          terminate: () => {},
        }),
        mainThreadBrowserTextMetricsRendererFactory: () => ({
          renderWithBrowserTextMetrics: mainThreadRenderWithBrowserTextMetrics,
        }),
        debounceMs: 10_000,
        stateStorage: {
          getItem: () => null,
          setItem: () => {},
        },
      });

      const debug = window.__mmdfluxDebug;
      const workerResult = await debug?.renderBrowserMetrics({
        input: multiFontInput,
        browserTextMetrics,
      });
      const mainThreadResult = await debug?.renderBrowserMetricsMainThread({
        input: multiFontInput,
        browserTextMetrics,
        show: false,
      });

      expect(workerResult?.source).toBe("worker-client");
      expect(mainThreadResult?.source).toBe("main-thread");
      expect(renderWithBrowserTextMetrics).toHaveBeenCalledWith(
        expect.objectContaining({
          browserTextMetrics: expect.objectContaining({
            textStyles: expect.arrayContaining([
              expect.objectContaining({
                fontFamily: "Verdana",
                cssFont: expect.stringContaining("Verdana"),
              }),
              expect.objectContaining({
                fontFamily: "Courier New",
                cssFont: expect.stringContaining("Courier New"),
              }),
              expect.objectContaining({
                fontFamily: "Times New Roman",
                cssFont: expect.stringContaining("Times New Roman"),
              }),
            ]),
          }),
        }),
      );
      expect(mainThreadRenderWithBrowserTextMetrics).toHaveBeenCalledWith(
        expect.objectContaining({
          browserTextMetrics: expect.objectContaining({
            textStyles: expect.arrayContaining([
              expect.objectContaining({
                fontFamily: "Verdana",
                cssFont: expect.stringContaining("Verdana"),
              }),
              expect.objectContaining({
                fontFamily: "Courier New",
                cssFont: expect.stringContaining("Courier New"),
              }),
              expect.objectContaining({
                fontFamily: "Times New Roman",
                cssFont: expect.stringContaining("Times New Roman"),
              }),
            ]),
          }),
        }),
      );
    } finally {
      history.replaceState(null, "", window.location.pathname);
      delete window.__mmdfluxDebug;
    }
  });
});
