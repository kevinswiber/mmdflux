import { describe, expect, it, vi } from "vitest";
import type { RenderWorkerClient } from "../src/main";
import { renderApp } from "../src/main";

async function flushTasks(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

// Keeps a test's format and fit settings out of the shared jsdom localStorage.
function createDetachedStorage() {
  return { getItem: () => null, setItem: () => {} };
}

function createFakeRenderClient() {
  const render = vi.fn(async (request) => ({
    seq: request.seq,
    format: request.format,
    output: `${request.format}:${request.input}`,
  }));

  return {
    render,
    renderWithBrowserTextMetrics: vi.fn(async (request) => ({
      seq: request.seq,
      format: "svg",
      output: `svg:${request.input}`,
    })),
    renderFitted: vi.fn(async (request) => ({
      seq: request.seq,
      format: request.format,
      output: `fitted:${request.input}`,
      fit: {
        outcome: "fitted",
        size: { width: 38, height: 20 },
        asAuthored: { width: 62, height: 14 },
        applied: [{ lever: "labelAwareSpacing" }],
      },
    })),
    validate: vi.fn(async () => '{"valid":true}'),
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

  it("limits the max width controls to text output", () => {
    const root = document.createElement("div");
    renderApp(root, {
      renderClientFactory: () => createFakeRenderClient(),
      debounceMs: 0,
      stateStorage: createDetachedStorage(),
    });

    const textTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="text"]',
    );
    const svgTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="svg"]',
    );
    const maxWidthInput =
      root.querySelector<HTMLInputElement>("[data-max-width]");
    const keepDirection = root.querySelector<HTMLInputElement>(
      "[data-fit-keep-direction]",
    );
    const truncate = root.querySelector<HTMLInputElement>(
      "[data-fit-truncate]",
    );
    const help = root.querySelector<HTMLElement>("[data-help-max-width]");
    if (
      !textTab ||
      !svgTab ||
      !maxWidthInput ||
      !keepDirection ||
      !truncate ||
      !help
    ) {
      throw new Error("expected max width controls");
    }

    svgTab.click();
    expect(maxWidthInput.disabled).toBe(true);
    expect(keepDirection.disabled).toBe(true);
    expect(truncate.disabled).toBe(true);
    expect(help.textContent).toContain("text output only");

    textTab.click();
    expect(maxWidthInput.disabled).toBe(false);
    expect(keepDirection.disabled).toBe(false);
    expect(truncate.disabled).toBe(false);
  });

  it("renders text through the fitted render once a max width is set", async () => {
    const root = document.createElement("div");
    const renderClient = createFakeRenderClient();
    renderApp(root, {
      renderClientFactory: () => renderClient,
      debounceMs: 0,
      stateStorage: createDetachedStorage(),
    });

    const textTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="text"]',
    );
    const svgTab = root.querySelector<HTMLButtonElement>(
      'button[data-format="svg"]',
    );
    const maxWidthInput =
      root.querySelector<HTMLInputElement>("[data-max-width]");
    const keepDirection = root.querySelector<HTMLInputElement>(
      "[data-fit-keep-direction]",
    );
    const truncate = root.querySelector<HTMLInputElement>(
      "[data-fit-truncate]",
    );
    const fitStatus = root.querySelector<HTMLElement>("[data-fit-status]");
    if (
      !textTab ||
      !svgTab ||
      !maxWidthInput ||
      !keepDirection ||
      !truncate ||
      !fitStatus
    ) {
      throw new Error("expected max width controls and fit status");
    }

    textTab.click();
    await flushTasks();
    expect(renderClient.renderFitted).not.toHaveBeenCalled();
    expect(fitStatus.hidden).toBe(true);

    maxWidthInput.value = "40";
    maxWidthInput.dispatchEvent(new Event("input"));
    await flushTasks();

    expect(renderClient.renderFitted).toHaveBeenLastCalledWith(
      expect.objectContaining({ format: "text", fitJson: '{"maxWidth":40}' }),
    );
    expect(fitStatus.hidden).toBe(false);
    expect(fitStatus.textContent).toBe(
      "Fitted to 40 columns: label-aware spacing; 38×20 (as authored 62×14).",
    );

    keepDirection.checked = true;
    keepDirection.dispatchEvent(new Event("change"));
    truncate.checked = true;
    truncate.dispatchEvent(new Event("change"));
    await flushTasks();
    expect(
      JSON.parse(renderClient.renderFitted.mock.lastCall?.[0].fitJson ?? ""),
    ).toEqual({ maxWidth: 40, fitDirection: "keep", truncate: true });

    renderClient.render.mockClear();
    renderClient.renderFitted.mockClear();
    svgTab.click();
    await flushTasks();
    expect(renderClient.renderFitted).not.toHaveBeenCalled();
    expect(fitStatus.hidden).toBe(true);

    textTab.click();
    await flushTasks();
    renderClient.renderFitted.mockClear();
    maxWidthInput.value = "";
    maxWidthInput.dispatchEvent(new Event("input"));
    await flushTasks();
    expect(renderClient.renderFitted).not.toHaveBeenCalled();
    expect(fitStatus.hidden).toBe(true);
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
      renderWithBrowserTextMetrics: vi.fn(async (request) => ({
        seq: request.seq,
        format: "svg",
        output: `svg:${request.input}`,
      })),
      validate: vi.fn(async () => '{"valid":true}'),
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
