import { describe, expect, it, vi } from "vitest";
import { createPreviewControls } from "./preview-controls";

function createPanzoomMock() {
  let scale = 1;
  let x = 0;
  let y = 0;
  return {
    destroy: vi.fn(),
    getPan: vi.fn(() => ({ x, y })),
    getScale: vi.fn(() => scale),
    pan: vi.fn((nextX: number, nextY: number) => {
      x = nextX;
      y = nextY;
    }),
    reset: vi.fn(() => {
      scale = 1;
      x = 0;
      y = 0;
    }),
    zoom: vi.fn((nextScale: number) => {
      scale = nextScale;
    }),
    zoomIn: vi.fn(() => {
      scale += 0.2;
    }),
    zoomOut: vi.fn(() => {
      scale -= 0.2;
    }),
    zoomWithWheel: vi.fn(),
  };
}

function createHarness(
  dependencies: Parameters<
    typeof createPreviewControls
  >[0]["dependencies"] = {},
) {
  const host = document.createElement("div");
  host.innerHTML = `
    <div data-controls hidden>
      <button type="button" data-zoom-out>-</button>
      <span data-zoom-label>100%</span>
      <button type="button" data-zoom-in>+</button>
      <button type="button" data-zoom-fit>Fit</button>
      <button type="button" data-zoom-reset>Reset</button>
    </div>
    <button type="button" data-export-toggle hidden>Export</button>
    <div data-export-menu hidden>
      <button type="button" data-export-svg>SVG</button>
      <button type="button" data-export-png>PNG</button>
    </div>
    <div data-output></div>
  `;

  const controlsRoot = host.querySelector<HTMLElement>("[data-controls]");
  const zoomOutButton =
    host.querySelector<HTMLButtonElement>("[data-zoom-out]");
  const zoomInButton = host.querySelector<HTMLButtonElement>("[data-zoom-in]");
  const zoomFitButton =
    host.querySelector<HTMLButtonElement>("[data-zoom-fit]");
  const zoomResetButton =
    host.querySelector<HTMLButtonElement>("[data-zoom-reset]");
  const zoomLabel = host.querySelector<HTMLElement>("[data-zoom-label]");
  const exportToggleButton = host.querySelector<HTMLButtonElement>(
    "[data-export-toggle]",
  );
  const exportMenu = host.querySelector<HTMLElement>("[data-export-menu]");
  const exportSvgButton =
    host.querySelector<HTMLButtonElement>("[data-export-svg]");
  const exportPngButton =
    host.querySelector<HTMLButtonElement>("[data-export-png]");
  const output = host.querySelector<HTMLElement>("[data-output]");

  if (
    !controlsRoot ||
    !zoomOutButton ||
    !zoomInButton ||
    !zoomFitButton ||
    !zoomResetButton ||
    !zoomLabel ||
    !exportToggleButton ||
    !exportMenu ||
    !exportSvgButton ||
    !exportPngButton ||
    !output
  ) {
    throw new Error("failed to create preview controls harness");
  }

  const controller = createPreviewControls({
    controlsRoot,
    zoomOutButton,
    zoomInButton,
    zoomFitButton,
    zoomResetButton,
    zoomLabel,
    exportToggleButton,
    exportMenu,
    exportSvgButton,
    exportPngButton,
    dependencies,
  });
  controller.attachTo(output);

  return {
    controller,
    controlsRoot,
    exportToggleButton,
    exportSvgButton,
    exportPngButton,
    output,
  };
}

describe("preview controls", () => {
  it("hides controls for non-SVG and shows them for SVG", () => {
    const panzoom = createPanzoomMock();
    const harness = createHarness({
      createPanzoom: () => panzoom,
    });

    harness.output.innerHTML = '<svg viewBox="0 0 100 80"></svg>';

    harness.controller.onResult("text");
    expect(harness.controlsRoot.hidden).toBe(true);
    expect(harness.exportToggleButton.hidden).toBe(true);

    harness.controller.onResult("svg");
    expect(harness.controlsRoot.hidden).toBe(false);
    expect(harness.exportToggleButton.hidden).toBe(false);

    harness.controller.dispose();
  });

  it("downloads SVG from export action", () => {
    const panzoom = createPanzoomMock();
    const createObjectUrl = vi.fn(() => "blob:svg");
    const revokeObjectUrl = vi.fn();

    const anchor = document.createElement("a");
    const anchorClick = vi.fn();
    anchor.click = anchorClick;

    let status = "";
    const harness = createHarness({
      createPanzoom: () => panzoom,
      createObjectUrl,
      revokeObjectUrl,
      createAnchor: () => anchor,
    });

    harness.controller.setStatusReporter((message) => {
      status = message;
    });

    harness.output.innerHTML =
      '<svg viewBox="0 0 120 90"><rect width="120" height="90" /></svg>';
    harness.controller.onResult("svg");

    harness.exportSvgButton.click();

    expect(createObjectUrl).toHaveBeenCalledTimes(1);
    expect(anchorClick).toHaveBeenCalledTimes(1);
    expect(status).toBe("Downloaded SVG.");

    harness.controller.dispose();
  });

  it("does not upscale initial fit above 100%", () => {
    const panzoom = createPanzoomMock();
    const harness = createHarness({
      createPanzoom: () => panzoom,
    });

    Object.defineProperty(harness.output, "clientWidth", {
      configurable: true,
      value: 620,
    });
    Object.defineProperty(harness.output, "clientHeight", {
      configurable: true,
      value: 420,
    });

    harness.output.innerHTML =
      '<svg viewBox="0 0 120 90"><rect width="120" height="90" /></svg>';
    harness.controller.onResult("svg");

    const latestScale = panzoom.zoom.mock.calls.at(-1)?.[0];
    expect(latestScale).toBe(1);

    harness.controller.dispose();
  });

  it("reports PNG conversion failures", async () => {
    const panzoom = createPanzoomMock();
    const createObjectUrl = vi.fn(() => "blob:svg-source");
    const revokeObjectUrl = vi.fn();

    const image = document.createElement("img");
    Object.defineProperty(image, "src", {
      set: () => {
        image.onload?.(new Event("load"));
      },
    });

    const canvas = {
      width: 0,
      height: 0,
      getContext: () => ({
        scale: vi.fn(),
        drawImage: vi.fn(),
      }),
      toBlob: (callback: BlobCallback) => {
        callback(null);
      },
    } as unknown as HTMLCanvasElement;

    let status = "";
    const harness = createHarness({
      createPanzoom: () => panzoom,
      createObjectUrl,
      revokeObjectUrl,
      createImage: () => image,
      createCanvas: () => canvas,
      devicePixelRatio: () => 1,
    });

    harness.controller.setStatusReporter((message) => {
      status = message;
    });

    harness.output.innerHTML =
      '<svg viewBox="0 0 120 90"><rect width="120" height="90" /></svg>';
    harness.controller.onResult("svg");

    harness.exportPngButton.click();
    await vi.waitFor(() => {
      expect(status).toContain("PNG export failed:");
    });
    expect(createObjectUrl).toHaveBeenCalled();
    expect(revokeObjectUrl).toHaveBeenCalled();

    harness.controller.dispose();
  });

  it("preserves viewport center anchor across SVG updates", () => {
    const panzoom = createPanzoomMock();
    const harness = createHarness({
      createPanzoom: () => panzoom,
    });

    Object.defineProperty(harness.output, "clientWidth", {
      configurable: true,
      value: 400,
    });
    Object.defineProperty(harness.output, "clientHeight", {
      configurable: true,
      value: 200,
    });

    harness.output.innerHTML =
      '<svg viewBox="0 0 200 100"><rect width="200" height="100" /></svg>';
    harness.controller.onResult("svg");

    panzoom.zoom(2);
    panzoom.pan(10, 20);

    harness.output.innerHTML =
      '<svg viewBox="0 0 400 200"><rect width="400" height="200" /></svg>';
    harness.controller.onResult("svg");

    const matchingPanCall = panzoom.pan.mock.calls.find(
      ([x, y]) => x === -80 && y === -10,
    );
    expect(matchingPanCall).toBeDefined();

    harness.controller.dispose();
  });
});
