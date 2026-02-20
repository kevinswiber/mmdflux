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
    const geometryLevelSelect = root.querySelector<HTMLSelectElement>(
      "[data-geometry-level]",
    );
    const pathDetailSelect =
      root.querySelector<HTMLSelectElement>("[data-path-detail]");

    if (
      !editorInput ||
      !mmdsTab ||
      !layoutEngineSelect ||
      !geometryLevelSelect ||
      !pathDetailSelect
    ) {
      throw new Error("expected editor input, format tab, and render controls");
    }

    editorInput.value = "graph TD\nA-->Saved";
    editorInput.dispatchEvent(new Event("input"));
    mmdsTab.click();
    layoutEngineSelect.value = "mermaid-layered";
    layoutEngineSelect.dispatchEvent(new Event("change"));
    geometryLevelSelect.value = "routed";
    geometryLevelSelect.dispatchEvent(new Event("change"));
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
      geometryLevel: "routed",
      pathDetail: "compact",
    });
  });
});
