import { describe, expect, it, vi } from "vitest";
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

describe("format-aware controls", () => {
  it("shows disabled-state reasons for format-specific controls", () => {
    const root = document.createElement("div");
    const renderClient = createFakeRenderClient();

    renderApp(root, {
      renderClientFactory: () => renderClient,
      debounceMs: 0,
    });

    const textTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="text"]',
    );
    const svgTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="svg"]',
    );
    const mmdsTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="mmds"]',
    );

    const edgePresetSelect =
      root.querySelector<HTMLSelectElement>("[data-edge-preset]");
    const pathDetailSelect =
      root.querySelector<HTMLSelectElement>("[data-path-detail]");

    const edgeHelp = root.querySelector<HTMLElement>("[data-help-edge-preset]");
    const pathHelp = root.querySelector<HTMLElement>("[data-help-path-detail]");
    const geometryLevelSelect = root.querySelector<HTMLSelectElement>(
      "[data-geometry-level]",
    );

    if (
      !textTab ||
      !svgTab ||
      !mmdsTab ||
      !edgePresetSelect ||
      !pathDetailSelect ||
      !edgeHelp ||
      !pathHelp
    ) {
      throw new Error("expected format controls and helper text elements");
    }

    expect(geometryLevelSelect).toBeNull();
    expect(edgePresetSelect.disabled).toBe(false);
    expect(pathDetailSelect.disabled).toBe(false);

    textTab.click();
    expect(edgePresetSelect.disabled).toBe(true);
    expect(pathDetailSelect.disabled).toBe(true);
    expect(edgeHelp.textContent).toContain("SVG output only");
    expect(pathHelp.textContent).toContain("SVG and MMDS");

    mmdsTab.click();
    expect(edgePresetSelect.disabled).toBe(true);
    expect(pathDetailSelect.disabled).toBe(false);
    expect(edgeHelp.textContent).toContain("SVG output only");

    svgTab.click();
    expect(edgePresetSelect.disabled).toBe(false);
    expect(pathDetailSelect.disabled).toBe(false);
  });

  it("toggles advanced panel without scheduling a render", () => {
    const root = document.createElement("div");
    const renderClient = createFakeRenderClient();

    renderApp(root, {
      renderClientFactory: () => renderClient,
      debounceMs: 0,
    });

    const advancedToggle = root.querySelector<HTMLButtonElement>(
      "[data-advanced-toggle]",
    );
    const advancedPanel = root.querySelector<HTMLElement>(
      "[data-advanced-panel]",
    );

    if (!advancedToggle || !advancedPanel) {
      throw new Error("expected advanced panel elements");
    }

    renderClient.render.mockClear();

    expect(advancedPanel.hidden).toBe(true);
    advancedToggle.click();
    expect(advancedPanel.hidden).toBe(false);
    advancedToggle.click();
    expect(advancedPanel.hidden).toBe(true);
    expect(renderClient.render).not.toHaveBeenCalled();
  });
});
