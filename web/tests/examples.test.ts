import { afterEach, describe, expect, it, vi } from "vitest";
import type { RenderWorkerClient } from "../src/main";
import { renderApp } from "../src/main";

function createFakeRenderClient() {
  const render = vi.fn(async (request) => ({
    seq: request.seq,
    format: request.format,
    output: `${request.format}:${request.input}`,
  }));
  return {
    render,
    terminate: vi.fn(),
  } satisfies RenderWorkerClient;
}

describe("playground examples", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders syntax-highlighted snippet previews", () => {
    const root = document.createElement("div");
    renderApp(root);

    const previews = [
      ...root.querySelectorAll<HTMLElement>(".snippet-preview"),
    ];

    expect(previews.length).toBeGreaterThan(0);
    for (const preview of previews) {
      expect(preview.querySelector(".snippet-token")).not.toBeNull();
    }
  });

  it("loads selected example into editor and triggers render", async () => {
    vi.useFakeTimers();
    const root = document.createElement("div");
    const renderClient = createFakeRenderClient();

    renderApp(root, {
      renderClientFactory: () => renderClient,
      debounceMs: 50,
    });

    const exampleSelect = root.querySelector<HTMLSelectElement>(
      "[data-example-select]",
    );
    const editorInput =
      root.querySelector<HTMLTextAreaElement>(".editor-input");

    if (!exampleSelect || !editorInput) {
      throw new Error("expected example select and editor input");
    }

    renderClient.render.mockClear();

    exampleSelect.value = "flowchart-subgraph-direction-override";
    exampleSelect.dispatchEvent(new Event("change"));
    vi.advanceTimersByTime(50);
    await Promise.resolve();

    expect(editorInput.value).toContain("subgraph lr_group");
    expect(renderClient.render).toHaveBeenCalledTimes(1);
    expect(renderClient.render.mock.calls[0]?.[0]).toMatchObject({
      format: "text",
    });
  });

  it("runs snippet cards in editor and triggers render", async () => {
    vi.useFakeTimers();
    const root = document.createElement("div");
    const renderClient = createFakeRenderClient();

    renderApp(root, {
      renderClientFactory: () => renderClient,
      debounceMs: 50,
    });

    const runButton = root.querySelector<HTMLButtonElement>(
      '[data-snippet-run="flowchart-subgraph-direction-override"]',
    );
    const editorInput =
      root.querySelector<HTMLTextAreaElement>(".editor-input");

    if (!runButton || !editorInput) {
      throw new Error("expected snippet run button and editor input");
    }

    renderClient.render.mockClear();

    runButton.click();
    vi.advanceTimersByTime(50);
    await Promise.resolve();

    expect(editorInput.value).toContain("subgraph lr_group");
    expect(renderClient.render).toHaveBeenCalledTimes(1);
    expect(renderClient.render.mock.calls[0]?.[0]).toMatchObject({
      format: "text",
    });
  });

  it("scrolls workspace into view when running snippets from off-screen", () => {
    const root = document.createElement("div");
    const renderClient = createFakeRenderClient();

    renderApp(root, {
      renderClientFactory: () => renderClient,
    });

    const runButton = root.querySelector<HTMLButtonElement>(
      '[data-snippet-run="flowchart-subgraph-direction-override"]',
    );
    const workspace = root.querySelector<HTMLElement>(".workspace");

    if (!runButton || !workspace) {
      throw new Error("expected snippet run button and workspace");
    }

    const scrollIntoView = vi.fn();
    workspace.scrollIntoView = scrollIntoView;
    Object.defineProperty(workspace, "getBoundingClientRect", {
      configurable: true,
      value: () => ({
        x: 0,
        y: -900,
        width: 1200,
        height: 700,
        top: -900,
        right: 1200,
        bottom: -200,
        left: 0,
        toJSON: () => ({}),
      }),
    });

    const originalMatchMedia = window.matchMedia;
    const originalInnerHeight = window.innerHeight;
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: vi.fn((query: string) => ({
        matches: false,
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    });
    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: 800,
    });

    runButton.click();

    expect(scrollIntoView).toHaveBeenCalledWith({
      behavior: "smooth",
      block: "start",
      inline: "nearest",
    });

    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: originalInnerHeight,
    });
    if (originalMatchMedia) {
      Object.defineProperty(window, "matchMedia", {
        configurable: true,
        value: originalMatchMedia,
      });
    } else {
      delete (window as Window & { matchMedia?: unknown }).matchMedia;
    }
  });

  it("does not scroll workspace when already mostly visible", () => {
    const root = document.createElement("div");
    const renderClient = createFakeRenderClient();

    renderApp(root, {
      renderClientFactory: () => renderClient,
    });

    const runButton = root.querySelector<HTMLButtonElement>(
      '[data-snippet-run="flowchart-subgraph-direction-override"]',
    );
    const workspace = root.querySelector<HTMLElement>(".workspace");

    if (!runButton || !workspace) {
      throw new Error("expected snippet run button and workspace");
    }

    const scrollIntoView = vi.fn();
    workspace.scrollIntoView = scrollIntoView;
    Object.defineProperty(workspace, "getBoundingClientRect", {
      configurable: true,
      value: () => ({
        x: 0,
        y: 20,
        width: 1200,
        height: 700,
        top: 20,
        right: 1200,
        bottom: 720,
        left: 0,
        toJSON: () => ({}),
      }),
    });

    const originalInnerHeight = window.innerHeight;
    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: 800,
    });

    runButton.click();

    expect(scrollIntoView).not.toHaveBeenCalled();

    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: originalInnerHeight,
    });
  });

  it("preserves current format while swapping examples", async () => {
    vi.useFakeTimers();
    const root = document.createElement("div");
    const renderClient = createFakeRenderClient();

    renderApp(root, {
      renderClientFactory: () => renderClient,
      debounceMs: 50,
    });

    const svgTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="svg"]',
    );
    const exampleSelect = root.querySelector<HTMLSelectElement>(
      "[data-example-select]",
    );

    if (!svgTab || !exampleSelect) {
      throw new Error("expected svg tab and example select");
    }

    svgTab.click();
    vi.advanceTimersByTime(50);
    await Promise.resolve();

    renderClient.render.mockClear();

    exampleSelect.value = "class-basics";
    exampleSelect.dispatchEvent(new Event("change"));
    vi.advanceTimersByTime(50);
    await Promise.resolve();

    expect(renderClient.render).toHaveBeenCalledTimes(1);
    expect(renderClient.render.mock.calls[0]?.[0]).toMatchObject({
      format: "svg",
    });
  });

  it("preserves current format when running snippet cards", async () => {
    vi.useFakeTimers();
    const root = document.createElement("div");
    const renderClient = createFakeRenderClient();

    renderApp(root, {
      renderClientFactory: () => renderClient,
      debounceMs: 50,
    });

    const svgTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="svg"]',
    );
    const runButton = root.querySelector<HTMLButtonElement>(
      '[data-snippet-run="class-basics"]',
    );

    if (!svgTab || !runButton) {
      throw new Error("expected svg tab and snippet run button");
    }

    svgTab.click();
    vi.advanceTimersByTime(50);
    await Promise.resolve();

    renderClient.render.mockClear();

    runButton.click();
    vi.advanceTimersByTime(50);
    await Promise.resolve();

    expect(renderClient.render).toHaveBeenCalledTimes(1);
    expect(renderClient.render.mock.calls[0]?.[0]).toMatchObject({
      format: "svg",
    });
  });

  it("sends render settings in configJson", async () => {
    vi.useFakeTimers();
    const root = document.createElement("div");
    const renderClient = createFakeRenderClient();

    renderApp(root, {
      renderClientFactory: () => renderClient,
      debounceMs: 50,
    });

    const svgTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="svg"]',
    );
    const layoutEngineSelect = root.querySelector<HTMLSelectElement>(
      "[data-layout-engine]",
    );
    const edgePresetSelect =
      root.querySelector<HTMLSelectElement>("[data-edge-preset]");
    const pathDetailSelect =
      root.querySelector<HTMLSelectElement>("[data-path-detail]");

    if (
      !svgTab ||
      !layoutEngineSelect ||
      !edgePresetSelect ||
      !pathDetailSelect
    ) {
      throw new Error("expected render setting controls");
    }

    renderClient.render.mockClear();

    svgTab.click();
    layoutEngineSelect.value = "mermaid-layered";
    layoutEngineSelect.dispatchEvent(new Event("change"));
    edgePresetSelect.value = "bezier";
    edgePresetSelect.dispatchEvent(new Event("change"));
    pathDetailSelect.value = "endpoints";
    pathDetailSelect.dispatchEvent(new Event("change"));

    vi.advanceTimersByTime(50);
    await Promise.resolve();

    const callCount = renderClient.render.mock.calls.length;
    expect(callCount).toBeGreaterThan(0);
    const payload = renderClient.render.mock.calls[callCount - 1]?.[0];
    expect(payload?.format).toBe("svg");
    expect(JSON.parse(payload?.configJson ?? "{}")).toEqual({
      layoutEngine: "mermaid-layered",
      edgePreset: "bezier",
      pathDetail: "endpoints",
    });
  });
});
