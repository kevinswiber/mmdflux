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

    const svgTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="svg"]',
    );
    const mmdsTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="mmds"]',
    );

    const edgePresetSelect =
      root.querySelector<HTMLSelectElement>("[data-edge-preset]");
    const geometryLevelSelect = root.querySelector<HTMLSelectElement>(
      "[data-geometry-level]",
    );
    const pathDetailSelect =
      root.querySelector<HTMLSelectElement>("[data-path-detail]");

    const edgeHelp = root.querySelector<HTMLElement>("[data-help-edge-preset]");
    const geometryHelp = root.querySelector<HTMLElement>(
      "[data-help-geometry-level]",
    );
    const pathHelp = root.querySelector<HTMLElement>("[data-help-path-detail]");

    if (
      !svgTab ||
      !mmdsTab ||
      !edgePresetSelect ||
      !geometryLevelSelect ||
      !pathDetailSelect ||
      !edgeHelp ||
      !geometryHelp ||
      !pathHelp
    ) {
      throw new Error("expected format controls and helper text elements");
    }

    expect(edgePresetSelect.disabled).toBe(true);
    expect(geometryLevelSelect.disabled).toBe(true);
    expect(pathDetailSelect.disabled).toBe(true);
    expect(edgeHelp.textContent).toContain("SVG output only");
    expect(geometryHelp.textContent).toContain("MMDS output only");
    expect(pathHelp.textContent).toContain("SVG and MMDS");

    svgTab.click();
    expect(edgePresetSelect.disabled).toBe(false);
    expect(geometryLevelSelect.disabled).toBe(true);
    expect(pathDetailSelect.disabled).toBe(false);

    mmdsTab.click();
    expect(edgePresetSelect.disabled).toBe(true);
    expect(geometryLevelSelect.disabled).toBe(false);
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
