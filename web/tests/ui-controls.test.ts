import { describe, expect, it, vi } from "vitest";
import type { RenderWorkerClient } from "../src/main";
import { renderApp } from "../src/main";

async function flushTasks(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

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
    const pathSimplificationSelect = root.querySelector<HTMLSelectElement>(
      "[data-path-simplification]",
    );

    const edgeHelp = root.querySelector<HTMLElement>("[data-help-edge-preset]");
    const pathHelp = root.querySelector<HTMLElement>(
      "[data-help-path-simplification]",
    );
    const geometryLevelSelect = root.querySelector<HTMLSelectElement>(
      "[data-geometry-level]",
    );

    if (
      !textTab ||
      !svgTab ||
      !mmdsTab ||
      !edgePresetSelect ||
      !pathSimplificationSelect ||
      !edgeHelp ||
      !pathHelp
    ) {
      throw new Error("expected format controls and helper text elements");
    }

    expect(geometryLevelSelect).toBeNull();
    expect(edgePresetSelect.disabled).toBe(false);
    expect(pathSimplificationSelect.disabled).toBe(false);

    textTab.click();
    expect(edgePresetSelect.disabled).toBe(true);
    expect(pathSimplificationSelect.disabled).toBe(true);
    expect(edgeHelp.textContent).toContain("SVG output only");
    expect(pathHelp.textContent).toContain("Path simplification");

    mmdsTab.click();
    expect(edgePresetSelect.disabled).toBe(true);
    expect(pathSimplificationSelect.disabled).toBe(false);
    expect(edgeHelp.textContent).toContain("SVG output only");

    svgTab.click();
    expect(edgePresetSelect.disabled).toBe(false);
    expect(pathSimplificationSelect.disabled).toBe(false);
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

  it("supports local text preview modes and copy actions without rerendering", async () => {
    const clipboard = {
      writeText: vi.fn(async () => {}),
    };
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: clipboard,
    });

    const root = document.createElement("div");
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

    renderApp(root, {
      renderClientFactory: () => renderClient,
      debounceMs: 0,
    });

    const textTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="text"]',
    );
    const previewOutput = root.querySelector<HTMLElement>(
      "[data-preview-output]",
    );
    const textToolbar = root.querySelector<HTMLElement>(
      "[data-text-preview-toolbar]",
    );
    const plainModeButton = root.querySelector<HTMLButtonElement>(
      'button[data-text-preview-mode="plain"]',
    );
    const styledModeButton = root.querySelector<HTMLButtonElement>(
      'button[data-text-preview-mode="styled"]',
    );
    const ansiModeButton = root.querySelector<HTMLButtonElement>(
      'button[data-text-preview-mode="ansi"]',
    );
    const copyPlainButton =
      root.querySelector<HTMLButtonElement>("[data-copy-plain]");
    const copyAnsiButton =
      root.querySelector<HTMLButtonElement>("[data-copy-ansi]");

    if (
      !textTab ||
      !previewOutput ||
      !textToolbar ||
      !plainModeButton ||
      !styledModeButton ||
      !ansiModeButton ||
      !copyPlainButton ||
      !copyAnsiButton
    ) {
      throw new Error("expected text preview toolbar and controls");
    }

    expect(textToolbar.hidden).toBe(true);

    textTab.click();
    await flushTasks();

    expect(textToolbar.hidden).toBe(false);
    expect(previewOutput.textContent).toBe("Alpha");

    const textRenderCall = renderClient.render.mock.calls.find(
      ([request]) => request.format === "text",
    )?.[0];
    expect(textRenderCall).toBeDefined();
    expect(JSON.parse(textRenderCall?.configJson ?? "{}")).toMatchObject({
      color: "always",
    });

    renderClient.render.mockClear();
    styledModeButton.click();
    expect(renderClient.render).not.toHaveBeenCalled();
    expect(previewOutput.querySelector("pre")?.textContent).toBe("Alpha");
    expect(previewOutput.querySelector("span")?.style.color).toBe(
      "rgb(255, 0, 0)",
    );

    ansiModeButton.click();
    expect(renderClient.render).not.toHaveBeenCalled();
    expect(previewOutput.textContent).toBe("\\x1b[38;2;255;0;0mAlpha\\x1b[0m");

    plainModeButton.click();
    expect(renderClient.render).not.toHaveBeenCalled();
    expect(previewOutput.textContent).toBe("Alpha");

    copyPlainButton.click();
    await flushTasks();
    expect(clipboard.writeText).toHaveBeenLastCalledWith("Alpha");

    copyAnsiButton.click();
    await flushTasks();
    expect(clipboard.writeText).toHaveBeenLastCalledWith(
      "\u001b[38;2;255;0;0mAlpha\u001b[0m",
    );
  });
});
