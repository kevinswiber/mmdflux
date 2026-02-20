import { describe, expect, it, vi } from "vitest";
import type { RenderWorkerClient } from "../src/main";
import { renderApp } from "../src/main";
import { encodeShareState } from "../src/share";

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
          pathDetail: "full",
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
        pathDetail: "full",
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
        pathDetail: "full",
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
    const pathDetailSelect =
      root.querySelector<HTMLSelectElement>("[data-path-detail]");

    if (!editorInput || !mmdsTab || !layoutEngineSelect || !pathDetailSelect) {
      throw new Error("expected editor input, format tab, and render controls");
    }

    editorInput.value = "graph TD\nA-->Saved";
    editorInput.dispatchEvent(new Event("input"));
    mmdsTab.click();
    layoutEngineSelect.value = "mermaid-layered";
    layoutEngineSelect.dispatchEvent(new Event("change"));
    pathDetailSelect.value = "compact";
    pathDetailSelect.dispatchEvent(new Event("change"));

    const persisted = JSON.parse(
      storage.getItem("mmdflux-playground-state") ?? "{}",
    ) as {
      v?: number;
      input?: string;
      format?: string;
      selectedExampleId?: string;
      customInput?: string;
      renderSettings?: Record<string, string>;
    };

    expect(persisted.v).toBe(3);
    expect(persisted.input).toBe("graph TD\nA-->Saved");
    expect(persisted.format).toBe("mmds");
    expect(persisted.selectedExampleId).toBe("__draft__");
    expect(persisted.customInput).toBe("graph TD\nA-->Saved");
    expect(persisted.renderSettings).toMatchObject({
      layoutEngine: "mermaid-layered",
      pathDetail: "compact",
    });
  });

  it("always emits routed geometry for MMDS config from legacy share settings", async () => {
    const shareHash = encodeShareState({
      input: "graph TD\nA-->B",
      format: "mmds",
      renderSettings: {
        layoutEngine: "auto",
        edgePreset: "auto",
        geometryLevel: "layout",
        pathDetail: "full",
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
});
