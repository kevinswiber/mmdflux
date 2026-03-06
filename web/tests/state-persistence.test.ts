import { describe, expect, it, vi } from "vitest";
import type { RenderWorkerClient } from "../src/main";
import { renderApp } from "../src/main";
import { decodeShareState, encodeShareState } from "../src/share";

interface MemoryStorage {
  getItem: (key: string) => string | null;
  setItem: (key: string, value: string) => void;
}

function createMemoryStorage(
  initialValues: Record<string, string> = {},
): MemoryStorage {
  const values = new Map(Object.entries(initialValues));
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => {
      values.set(key, value);
    },
  };
}

function createFakeRenderClient() {
  return {
    render: vi.fn(async (request) => ({
      seq: request.seq,
      format: request.format,
      output: `${request.format}:${request.input}`,
    })),
    terminate: vi.fn(),
  } satisfies RenderWorkerClient;
}

async function flushTasks(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe("playground state persistence", () => {
  it("defaults format to SVG when no share or persisted format exists", () => {
    const root = document.createElement("div");

    renderApp(root, {
      renderClientFactory: () => createFakeRenderClient(),
      stateStorage: createMemoryStorage(),
    });

    const activeTab = root.querySelector<HTMLButtonElement>(
      ".format-tabs button.is-active",
    );

    expect(activeTab?.dataset.format).toBe("svg");
  });

  it("labels restored local draft content as Draft", () => {
    const storage = createMemoryStorage({
      "mmdflux-playground-state": JSON.stringify({
        v: 2,
        input: "graph TD\nLocalCustom-->State",
        format: "text",
        renderSettings: {
          layoutEngine: "auto",
          edgePreset: "auto",
          geometryLevel: "layout",
          pathSimplification: "none",
        },
      }),
    });
    const root = document.createElement("div");

    renderApp(root, {
      renderClientFactory: () => createFakeRenderClient(),
      stateStorage: storage,
    });

    const exampleSelect = root.querySelector<HTMLSelectElement>(
      "[data-example-select]",
    );
    const draftOption = root.querySelector<HTMLOptionElement>(
      '[data-example-select] option[value="__draft__"]',
    );

    expect(exampleSelect?.value).toBe("__draft__");
    expect(draftOption?.textContent).toBe("Draft");
  });

  it("labels hash-restored custom content as Draft", () => {
    const shareHash = encodeShareState({
      input: "graph TD\nHashCustom-->State",
      format: "text",
      renderSettings: {
        layoutEngine: "auto",
        edgePreset: "auto",
        geometryLevel: "layout",
        pathSimplification: "none",
      },
    });
    try {
      history.replaceState(null, "", `#${shareHash}`);

      const root = document.createElement("div");
      renderApp(root, {
        renderClientFactory: () => createFakeRenderClient(),
        stateStorage: createMemoryStorage(),
      });

      const exampleSelect = root.querySelector<HTMLSelectElement>(
        "[data-example-select]",
      );
      const draftOption = root.querySelector<HTMLOptionElement>(
        '[data-example-select] option[value="__draft__"]',
      );

      expect(exampleSelect?.value).toBe("__draft__");
      expect(draftOption?.textContent).toBe("Draft");
    } finally {
      history.replaceState(null, "", window.location.pathname);
    }
  });

  it("restores editor input and format from persisted state", () => {
    const storage = createMemoryStorage({
      "mmdflux-playground-state": JSON.stringify({
        v: 1,
        input: "graph LR\nPersisted-->State",
        format: "svg",
      }),
    });
    const root = document.createElement("div");

    renderApp(root, {
      renderClientFactory: () => createFakeRenderClient(),
      stateStorage: storage,
    });

    const editorInput =
      root.querySelector<HTMLTextAreaElement>(".editor-input");
    const activeTab = root.querySelector<HTMLButtonElement>(
      ".format-tabs button.is-active",
    );

    expect(editorInput?.value).toContain("Persisted-->State");
    expect(activeTab?.dataset.format).toBe("svg");
  });

  it("honors share format instead of default SVG fallback", () => {
    const shareHash = encodeShareState({
      input: "graph TD\nShareFormat-->Text",
      format: "text",
      renderSettings: {
        layoutEngine: "auto",
        edgePreset: "auto",
        geometryLevel: "layout",
        pathSimplification: "none",
      },
    });
    try {
      history.replaceState(null, "", `#${shareHash}`);

      const root = document.createElement("div");
      renderApp(root, {
        renderClientFactory: () => createFakeRenderClient(),
        stateStorage: createMemoryStorage(),
      });

      const activeTab = root.querySelector<HTMLButtonElement>(
        ".format-tabs button.is-active",
      );
      expect(activeTab?.dataset.format).toBe("text");
    } finally {
      history.replaceState(null, "", window.location.pathname);
    }
  });

  it("persists latest editor input and selected format on change", () => {
    const storage = createMemoryStorage();
    const root = document.createElement("div");

    renderApp(root, {
      renderClientFactory: () => createFakeRenderClient(),
      stateStorage: storage,
    });

    const editorInput =
      root.querySelector<HTMLTextAreaElement>(".editor-input");
    const mmdsTab = root.querySelector<HTMLButtonElement>(
      '.format-tabs button[data-format="mmds"]',
    );
    const layoutEngineSelect = root.querySelector<HTMLSelectElement>(
      "[data-layout-engine]",
    );
    const pathSimplificationSelect = root.querySelector<HTMLSelectElement>(
      "[data-path-simplification]",
    );

    if (
      !editorInput ||
      !mmdsTab ||
      !layoutEngineSelect ||
      !pathSimplificationSelect
    ) {
      throw new Error("expected editor input, format tab, and render controls");
    }

    editorInput.value = "graph TD\nA-->Saved";
    editorInput.dispatchEvent(new Event("input"));
    mmdsTab.click();
    layoutEngineSelect.value = "mermaid-layered";
    layoutEngineSelect.dispatchEvent(new Event("change"));
    pathSimplificationSelect.value = "lossless";
    pathSimplificationSelect.dispatchEvent(new Event("change"));

    const persisted = JSON.parse(
      storage.getItem("mmdflux-playground-state") ?? "{}",
    ) as {
      v?: number;
      input?: string;
      format?: string;
      textPreviewMode?: string;
      selectedExampleId?: string;
      customInput?: string;
      renderSettings?: Record<string, string>;
    };

    expect(persisted.v).toBe(4);
    expect(persisted.input).toBe("graph TD\nA-->Saved");
    expect(persisted.format).toBe("mmds");
    expect(persisted.textPreviewMode).toBe("plain");
    expect(persisted.selectedExampleId).toBe("__draft__");
    expect(persisted.customInput).toBe("graph TD\nA-->Saved");
    expect(persisted.renderSettings).toMatchObject({
      layoutEngine: "mermaid-layered",
      pathSimplification: "lossless",
    });
  });

  it("persists and restores text preview mode in local state", async () => {
    const storage = createMemoryStorage();
    const renderClient = {
      render: vi.fn(async (request) => ({
        seq: request.seq,
        format: request.format,
        output:
          request.format === "text"
            ? "\u001b[38;2;255;0;0mAlpha\u001b[0m"
            : `${request.format}:${request.input}`,
      })),
      terminate: vi.fn(),
    } satisfies RenderWorkerClient;

    const root = document.createElement("div");
    renderApp(root, {
      renderClientFactory: () => renderClient,
      debounceMs: 0,
      stateStorage: storage,
    });

    const textTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="text"]',
    );
    const ansiModeButton = root.querySelector<HTMLButtonElement>(
      'button[data-text-preview-mode="ansi"]',
    );

    if (!textTab || !ansiModeButton) {
      throw new Error("expected text tab and ANSI preview mode button");
    }

    textTab.click();
    await flushTasks();
    ansiModeButton.click();

    const persisted = JSON.parse(
      storage.getItem("mmdflux-playground-state") ?? "{}",
    ) as {
      v?: number;
      textPreviewMode?: string;
    };
    expect(persisted.v).toBe(4);
    expect(persisted.textPreviewMode).toBe("ansi");

    const restoredRoot = document.createElement("div");
    renderApp(restoredRoot, {
      renderClientFactory: () => renderClient,
      debounceMs: 0,
      stateStorage: storage,
    });
    await flushTasks();

    const restoredAnsiButton = restoredRoot.querySelector<HTMLButtonElement>(
      'button[data-text-preview-mode="ansi"]',
    );
    const previewOutput = restoredRoot.querySelector<HTMLElement>(
      "[data-preview-output]",
    );

    expect(restoredAnsiButton?.classList.contains("is-active")).toBe(true);
    expect(previewOutput?.textContent).toBe("\\x1b[38;2;255;0;0mAlpha\\x1b[0m");
  });

  it("serializes text preview mode into share URLs and restores it from hash", async () => {
    const clipboard = {
      writeText: vi.fn(async () => {}),
    };
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: clipboard,
    });

    try {
      history.replaceState(null, "", window.location.pathname);

      const storage = createMemoryStorage();
      const root = document.createElement("div");
      renderApp(root, {
        renderClientFactory: () => createFakeRenderClient(),
        debounceMs: 0,
        stateStorage: storage,
      });

      const textTab = root.querySelector<HTMLButtonElement>(
        'button[data-format="text"]',
      );
      const ansiModeButton = root.querySelector<HTMLButtonElement>(
        'button[data-text-preview-mode="ansi"]',
      );
      const shareButton = root.querySelector<HTMLButtonElement>("[data-share]");

      if (!textTab || !ansiModeButton || !shareButton) {
        throw new Error(
          "expected text tab, ANSI preview button, and share button",
        );
      }

      textTab.click();
      await flushTasks();
      ansiModeButton.click();
      shareButton.click();
      await flushTasks();

      const copiedShareUrl = clipboard.writeText.mock.calls[0]?.[0] as
        | string
        | undefined;
      expect(copiedShareUrl).toBeDefined();

      const shareHash = new URL(copiedShareUrl ?? window.location.href).hash;
      const decoded = decodeShareState(shareHash);
      expect(decoded?.textPreviewMode).toBe("ansi");

      history.replaceState(null, "", shareHash);

      const restoredRoot = document.createElement("div");
      renderApp(restoredRoot, {
        renderClientFactory: () => createFakeRenderClient(),
        debounceMs: 0,
        stateStorage: createMemoryStorage(),
      });

      const restoredAnsiButton = restoredRoot.querySelector<HTMLButtonElement>(
        'button[data-text-preview-mode="ansi"]',
      );
      expect(restoredAnsiButton?.classList.contains("is-active")).toBe(true);
    } finally {
      history.replaceState(null, "", window.location.pathname);
    }
  });

  it("always emits routed geometry for MMDS config from legacy share settings", async () => {
    const shareHash = encodeShareState({
      input: "graph TD\nA-->B",
      format: "mmds",
      renderSettings: {
        layoutEngine: "auto",
        edgePreset: "auto",
        geometryLevel: "layout",
        pathSimplification: "none",
      },
    });
    const renderClient = createFakeRenderClient();

    try {
      history.replaceState(null, "", `#${shareHash}`);

      const root = document.createElement("div");
      renderApp(root, {
        renderClientFactory: () => renderClient,
        debounceMs: 0,
        stateStorage: createMemoryStorage(),
      });

      await Promise.resolve();

      expect(renderClient.render).toHaveBeenCalledTimes(1);
      const request = renderClient.render.mock.calls[0]?.[0] as {
        format: string;
        configJson?: string;
      };
      expect(request.format).toBe("mmds");
      expect(JSON.parse(request.configJson ?? "{}")).toMatchObject({
        geometryLevel: "routed",
      });
    } finally {
      history.replaceState(null, "", window.location.pathname);
    }
  });

  it("maps MMDS path simplification to config", async () => {
    const shareHash = encodeShareState({
      input: "graph TD\nA-->B",
      format: "mmds",
      renderSettings: {
        layoutEngine: "auto",
        edgePreset: "auto",
        geometryLevel: "layout",
        pathSimplification: "lossless",
      },
    });
    const renderClient = createFakeRenderClient();

    try {
      history.replaceState(null, "", `#${shareHash}`);

      const root = document.createElement("div");
      renderApp(root, {
        renderClientFactory: () => renderClient,
        debounceMs: 0,
        stateStorage: createMemoryStorage(),
      });

      await Promise.resolve();

      expect(renderClient.render).toHaveBeenCalledTimes(1);
      const request = renderClient.render.mock.calls[0]?.[0] as {
        format: string;
        configJson?: string;
      };
      expect(request.format).toBe("mmds");
      const config = JSON.parse(request.configJson ?? "{}") as Record<
        string,
        string
      >;
      expect(config).toMatchObject({
        geometryLevel: "routed",
        pathSimplification: "lossless",
      });
      expect(config.pathDetail).toBeUndefined();
    } finally {
      history.replaceState(null, "", window.location.pathname);
    }
  });

  it("maps legacy pathDetail share values to pathSimplification", async () => {
    const legacyPayload = {
      v: 2,
      input: "graph TD\nA-->B",
      format: "mmds",
      renderSettings: {
        layoutEngine: "auto",
        edgePreset: "auto",
        geometryLevel: "layout",
        pathDetail: "compact",
      },
    };
    const shareHash = btoa(JSON.stringify(legacyPayload))
      .replaceAll("+", "-")
      .replaceAll("/", "_")
      .replaceAll("=", "");
    const renderClient = createFakeRenderClient();

    try {
      history.replaceState(null, "", `#${shareHash}`);

      const root = document.createElement("div");
      renderApp(root, {
        renderClientFactory: () => renderClient,
        debounceMs: 0,
        stateStorage: createMemoryStorage(),
      });

      await Promise.resolve();

      expect(renderClient.render).toHaveBeenCalledTimes(1);
      const request = renderClient.render.mock.calls[0]?.[0] as {
        configJson?: string;
      };
      const config = JSON.parse(request.configJson ?? "{}") as Record<
        string,
        string
      >;
      expect(config.pathSimplification).toBe("lossless");
    } finally {
      history.replaceState(null, "", window.location.pathname);
    }
  });
});
