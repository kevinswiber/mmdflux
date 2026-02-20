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
    zoomFitButton,
    zoomResetButton,
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

  it("auto-fits newly rendered SVG updates", () => {
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

    const pan = panzoom.getPan();
    expect(panzoom.getScale()).toBe(1);
    expect(pan.x).toBe(0);
    expect(pan.y).toBe(0);

    harness.controller.dispose();
  });

  it("keeps fit stable when clicked repeatedly", () => {
    const panzoom = createPanzoomMock();
    const harness = createHarness({
      createPanzoom: () => panzoom,
    });

    Object.defineProperty(harness.output, "clientWidth", {
      configurable: true,
      value: 500,
    });
    Object.defineProperty(harness.output, "clientHeight", {
      configurable: true,
      value: 300,
    });

    harness.output.innerHTML =
      '<svg viewBox="0 0 1200 400"><rect width="1200" height="400" /></svg>';
    harness.controller.onResult("svg");

    panzoom.zoom(2);
    panzoom.pan(-300, -120);

    harness.zoomFitButton.click();
    const firstScale = panzoom.getScale();
    const firstPan = panzoom.getPan();

    harness.zoomFitButton.click();
    const secondScale = panzoom.getScale();
    const secondPan = panzoom.getPan();

    expect(secondScale).toBe(firstScale);
    expect(secondPan.x).toBe(firstPan.x);
    expect(secondPan.y).toBe(firstPan.y);

    harness.controller.dispose();
  });

  it("reset centers large diagrams at 100% zoom", () => {
    const panzoom = createPanzoomMock();
    const harness = createHarness({
      createPanzoom: () => panzoom,
    });

    Object.defineProperty(harness.output, "clientWidth", {
      configurable: true,
      value: 500,
    });
    Object.defineProperty(harness.output, "clientHeight", {
      configurable: true,
      value: 300,
    });

    harness.output.innerHTML =
      '<svg viewBox="0 0 3000 1200"><rect width="3000" height="1200" /></svg>';
    harness.controller.onResult("svg");

    panzoom.zoom(3);
    panzoom.pan(-700, -250);

    harness.zoomResetButton.click();

    const pan = panzoom.getPan();
    expect(panzoom.getScale()).toBe(1);
    expect(pan.x).toBe(-1250);
    expect(pan.y).toBe(-450);

    harness.controller.dispose();
  });

  it("centers fit and reset for offset viewBox origins", () => {
    const panzoom = createPanzoomMock();
    const harness = createHarness({
      createPanzoom: () => panzoom,
    });

    Object.defineProperty(harness.output, "clientWidth", {
      configurable: true,
      value: 500,
    });
    Object.defineProperty(harness.output, "clientHeight", {
      configurable: true,
      value: 300,
    });

    harness.output.innerHTML =
      '<svg viewBox="-100 -50 200 100"><rect x="-100" y="-50" width="200" height="100" /></svg>';
    harness.controller.onResult("svg");

    const fitPan = panzoom.getPan();
    expect(fitPan.x).toBe(250);
    expect(fitPan.y).toBe(150);

    panzoom.zoom(1.8);
    panzoom.pan(10, 20);
    harness.zoomResetButton.click();

    const resetPan = panzoom.getPan();
    expect(resetPan.x).toBe(250);
    expect(resetPan.y).toBe(150);
    expect(panzoom.getScale()).toBe(1);

    harness.controller.dispose();
  });

  it("uses viewBox bounds even when content bbox is offset", () => {
    const panzoom = createPanzoomMock();
    const harness = createHarness({
      createPanzoom: () => panzoom,
    });

    Object.defineProperty(harness.output, "clientWidth", {
      configurable: true,
      value: 500,
    });
    Object.defineProperty(harness.output, "clientHeight", {
      configurable: true,
      value: 300,
    });

    harness.output.innerHTML =
      '<svg viewBox="0 0 1000 200"><g class="root"><rect x="0" y="0" width="1000" height="200" /></g></svg>';

    const svg = harness.output.querySelector("svg");
    const root = harness.output.querySelector("g.root");
    if (!svg || !root) {
      throw new Error("failed to create svg test fixture");
    }

    Object.defineProperty(root, "getBBox", {
      configurable: true,
      value: () => ({ x: 400, y: 0, width: 400, height: 200 }),
    });
    Object.defineProperty(svg, "getBBox", {
      configurable: true,
      value: () => ({ x: 400, y: 0, width: 400, height: 200 }),
    });

    harness.controller.onResult("svg");

    const pan = panzoom.getPan();
    expect(panzoom.getScale()).toBe(0.5);
    expect(pan.x).toBe(0);
    expect(pan.y).toBe(200);

    harness.controller.dispose();
  });

  it("parses viewBox attribute when animated viewBox values are unavailable", () => {
    const panzoom = createPanzoomMock();
    const harness = createHarness({
      createPanzoom: () => panzoom,
    });

    Object.defineProperty(harness.output, "clientWidth", {
      configurable: true,
      value: 500,
    });
    Object.defineProperty(harness.output, "clientHeight", {
      configurable: true,
      value: 300,
    });

    harness.output.innerHTML =
      '<svg viewBox="0 0 1000 200" width="100%" style="max-width: 1000px"><g class="root"><rect x="0" y="0" width="1000" height="200" /></g></svg>';

    const svg = harness.output.querySelector("svg");
    if (!svg) {
      throw new Error("failed to create svg test fixture");
    }

    Object.defineProperty(svg, "viewBox", {
      configurable: true,
      value: {
        baseVal: {
          x: 0,
          y: 0,
          width: 0,
          height: 0,
        },
      },
    });

    harness.controller.onResult("svg");

    const pan = panzoom.getPan();
    expect(panzoom.getScale()).toBe(0.5);
    expect(pan.x).toBe(0);
    expect(pan.y).toBe(200);

    harness.controller.dispose();
  });
});
