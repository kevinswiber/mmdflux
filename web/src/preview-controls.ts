import Panzoom from "@panzoom/panzoom";

import type { WorkerOutputFormat } from "./worker-protocol";

interface PanzoomInstance {
  destroy: () => void;
  getPan: () => { x: number; y: number };
  getScale: () => number;
  pan: (x: number, y: number, options?: Record<string, unknown>) => void;
  reset: () => void;
  zoom: (scale: number, options?: Record<string, unknown>) => void;
  zoomIn: (options?: Record<string, unknown>) => void;
  zoomOut: (options?: Record<string, unknown>) => void;
  zoomWithWheel: (event: WheelEvent) => void;
}

type StatusReporter = (message: string) => void;

interface ViewAnchor {
  centerXRatio: number;
  centerYRatio: number;
  scale: number;
}

interface DiagramBounds {
  minX: number;
  minY: number;
  width: number;
  height: number;
}

interface PreviewControlDependencies {
  createPanzoom: (target: SVGElement) => PanzoomInstance;
  createObjectUrl: (blob: Blob) => string;
  revokeObjectUrl: (url: string) => void;
  createAnchor: () => HTMLAnchorElement;
  createImage: () => HTMLImageElement;
  createCanvas: () => HTMLCanvasElement;
  devicePixelRatio: () => number;
}

interface CreatePreviewControlsOptions {
  controlsRoot: HTMLElement;
  zoomOutButton: HTMLButtonElement;
  zoomInButton: HTMLButtonElement;
  zoomFitButton: HTMLButtonElement;
  zoomResetButton: HTMLButtonElement;
  zoomLabel: HTMLElement;
  exportToggleButton: HTMLButtonElement;
  exportMenu: HTMLElement;
  exportSvgButton: HTMLButtonElement;
  exportPngButton: HTMLButtonElement;
  dependencies?: Partial<PreviewControlDependencies>;
}

export interface PreviewControlsController {
  attachTo: (outputRoot: HTMLElement) => void;
  onResult: (format: WorkerOutputFormat) => void;
  setStatusReporter: (reporter: StatusReporter) => void;
  dispose: () => void;
}

const MIN_SCALE = 0.2;
const MAX_SCALE = 20;
const ZOOM_STEP = 0.2;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function readCssPixels(value: string): number {
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function readNumericAttribute(
  svg: SVGSVGElement,
  name: "width" | "height",
): number {
  const raw = svg.getAttribute(name);
  if (!raw) {
    return 0;
  }

  const trimmed = raw.trim();
  if (!trimmed || trimmed.endsWith("%")) {
    return 0;
  }

  if (!/^-?\d+(\.\d+)?([a-zA-Z]+)?$/.test(trimmed)) {
    return 0;
  }

  const parsed = Number.parseFloat(raw);
  return Number.isFinite(parsed) ? parsed : 0;
}

function parseViewBoxAttribute(svg: SVGSVGElement): DiagramBounds | null {
  const raw = svg.getAttribute("viewBox");
  if (!raw) {
    return null;
  }

  const parts = raw
    .trim()
    .split(/[,\s]+/)
    .map((value) => Number.parseFloat(value));
  if (
    parts.length !== 4 ||
    parts.some((value) => !Number.isFinite(value)) ||
    parts[2] <= 0 ||
    parts[3] <= 0
  ) {
    return null;
  }

  return {
    minX: parts[0],
    minY: parts[1],
    width: parts[2],
    height: parts[3],
  };
}

function getSvgDimensions(svg: SVGSVGElement): {
  width: number;
  height: number;
} {
  const bounds = getSvgBounds(svg);
  return { width: bounds.width, height: bounds.height };
}

function getSvgBounds(svg: SVGSVGElement): DiagramBounds {
  const attributeViewBox = parseViewBoxAttribute(svg);
  if (attributeViewBox) {
    return attributeViewBox;
  }

  const viewBox = svg.viewBox.baseVal;
  if (viewBox.width > 0 && viewBox.height > 0) {
    return {
      minX: viewBox.x,
      minY: viewBox.y,
      width: viewBox.width,
      height: viewBox.height,
    };
  }

  const width = readNumericAttribute(svg, "width");
  const height = readNumericAttribute(svg, "height");
  if (width > 0 && height > 0) {
    return {
      minX: 0,
      minY: 0,
      width,
      height,
    };
  }

  const rect = svg.getBoundingClientRect();
  return {
    minX: 0,
    minY: 0,
    width: Math.max(rect.width, 1),
    height: Math.max(rect.height, 1),
  };
}

function hasUsableViewBox(svg: SVGSVGElement): boolean {
  if (parseViewBoxAttribute(svg)) {
    return true;
  }

  const viewBox = svg.viewBox.baseVal;
  return viewBox.width > 0 && viewBox.height > 0;
}

function toFiniteBounds(bounds: {
  x: number;
  y: number;
  width: number;
  height: number;
}): DiagramBounds | null {
  if (
    !Number.isFinite(bounds.x) ||
    !Number.isFinite(bounds.y) ||
    !Number.isFinite(bounds.width) ||
    !Number.isFinite(bounds.height) ||
    bounds.width <= 0 ||
    bounds.height <= 0
  ) {
    return null;
  }

  return {
    minX: bounds.x,
    minY: bounds.y,
    width: bounds.width,
    height: bounds.height,
  };
}

function getSvgContentBounds(svg: SVGSVGElement): DiagramBounds | null {
  const rootGroup = svg.querySelector<SVGGraphicsElement>("g.root");
  if (rootGroup) {
    try {
      const rootBounds = toFiniteBounds(rootGroup.getBBox());
      if (rootBounds) {
        return rootBounds;
      }
    } catch {
      // ignore and fall through
    }
  }

  try {
    const svgBounds = toFiniteBounds(svg.getBBox());
    if (svgBounds) {
      return svgBounds;
    }
  } catch {
    // ignore and fall through
  }

  return null;
}

function getDiagramBounds(svg: SVGSVGElement): DiagramBounds {
  const svgBounds = getSvgBounds(svg);
  if (hasUsableViewBox(svg)) {
    return svgBounds;
  }

  return getSvgContentBounds(svg) ?? svgBounds;
}

function getViewportDimensions(element: HTMLElement): {
  width: number;
  height: number;
} {
  const style = window.getComputedStyle(element);
  const paddingX =
    readCssPixels(style.paddingLeft) + readCssPixels(style.paddingRight);
  const paddingY = readCssPixels(style.paddingTop) + readCssPixels(style.paddingBottom);

  return {
    width: Math.max(0, element.clientWidth - paddingX),
    height: Math.max(0, element.clientHeight - paddingY),
  };
}

function clampRatio(value: number): number {
  return clamp(value, 0, 1);
}

function captureViewAnchor(
  outputRoot: HTMLElement,
  panzoom: PanzoomInstance,
  bounds: DiagramBounds,
): ViewAnchor | null {
  const { width: viewportWidth, height: viewportHeight } =
    getViewportDimensions(outputRoot);
  const { minX, minY, width, height } = bounds;
  if (viewportWidth <= 0 || viewportHeight <= 0 || width <= 0 || height <= 0) {
    return null;
  }

  const scale = panzoom.getScale();
  if (!Number.isFinite(scale) || scale <= 0) {
    return null;
  }

  const pan = panzoom.getPan();
  const centerX = viewportWidth / (2 * scale) - pan.x;
  const centerY = viewportHeight / (2 * scale) - pan.y;
  return {
    centerXRatio: clampRatio((centerX - minX) / width),
    centerYRatio: clampRatio((centerY - minY) / height),
    scale,
  };
}

function normalizeSvgSize(svg: SVGSVGElement): DiagramBounds {
  const bounds = getSvgBounds(svg);
  if (bounds.width > 0 && bounds.height > 0) {
    svg.setAttribute("width", String(bounds.width));
    svg.setAttribute("height", String(bounds.height));
  }
  return bounds;
}

function panForCenteredView(
  viewportWidth: number,
  viewportHeight: number,
  bounds: DiagramBounds,
  scale: number,
): { x: number; y: number } {
  const centerX = bounds.minX + bounds.width / 2;
  const centerY = bounds.minY + bounds.height / 2;
  return {
    x: viewportWidth / (2 * scale) - centerX,
    y: viewportHeight / (2 * scale) - centerY,
  };
}

function serializeSvg(svg: SVGSVGElement): string {
  const clone = svg.cloneNode(true) as SVGSVGElement;
  if (!clone.getAttribute("xmlns")) {
    clone.setAttribute("xmlns", "http://www.w3.org/2000/svg");
  }
  if (!clone.getAttribute("xmlns:xlink")) {
    clone.setAttribute("xmlns:xlink", "http://www.w3.org/1999/xlink");
  }
  return new XMLSerializer().serializeToString(clone);
}

function percentageLabel(scale: number): string {
  return `${Math.round(scale * 100)}%`;
}

function defaultDependencies(): PreviewControlDependencies {
  return {
    createPanzoom: (target) =>
      Panzoom(target, {
        canvas: true,
        maxScale: MAX_SCALE,
        minScale: MIN_SCALE,
        origin: "0 0",
        roundPixels: true,
      }) as unknown as PanzoomInstance,
    createObjectUrl: (blob) => URL.createObjectURL(blob),
    revokeObjectUrl: (url) => URL.revokeObjectURL(url),
    createAnchor: () => document.createElement("a"),
    createImage: () => new Image(),
    createCanvas: () => document.createElement("canvas"),
    devicePixelRatio: () => window.devicePixelRatio || 1,
  };
}

export function createPreviewControls(
  options: CreatePreviewControlsOptions,
): PreviewControlsController {
  const dependencies: PreviewControlDependencies = {
    ...defaultDependencies(),
    ...options.dependencies,
  };
  let reportStatus: StatusReporter = () => {};
  let outputRoot: HTMLElement | null = null;
  let currentFormat: WorkerOutputFormat = "text";
  let currentSvg: SVGSVGElement | null = null;
  let currentPanTarget: SVGElement | null = null;
  let currentPanTargetBounds: DiagramBounds | null = null;
  let panzoom: PanzoomInstance | null = null;
  let wheelHost: HTMLElement | null = null;
  let fitTicket = 0;
  let viewAnchor: ViewAnchor | null = null;

  const forceSvgRepaint = (): void => {
    if (!currentSvg) {
      return;
    }

    const previousOutline = currentSvg.style.outline;
    currentSvg.style.outline = "1px solid transparent";
    void currentSvg.getBoundingClientRect();
    currentSvg.style.outline = previousOutline;
  };

  const resetZoomLabel = (): void => {
    options.zoomLabel.textContent = percentageLabel(1);
  };

  const updateZoomLabel = (): void => {
    if (!panzoom) {
      resetZoomLabel();
      return;
    }

    options.zoomLabel.textContent = percentageLabel(panzoom.getScale());
  };

  const teardownPanzoom = (): void => {
    if (panzoom && outputRoot && currentPanTargetBounds) {
      viewAnchor = captureViewAnchor(outputRoot, panzoom, currentPanTargetBounds);
    }
    fitTicket += 1;
    if (wheelHost) {
      wheelHost.removeEventListener("wheel", handleWheel as EventListener);
    }
    wheelHost = null;
    panzoom?.destroy();
    panzoom = null;
    currentPanTarget = null;
    currentPanTargetBounds = null;
    currentSvg = null;
    resetZoomLabel();
  };

  const resolveSvgFromOutput = (): SVGSVGElement | null => {
    if (!outputRoot) {
      return null;
    }
    return outputRoot.querySelector<SVGSVGElement>("svg");
  };

  const shouldShowControls = (): boolean => {
    return currentFormat === "svg" && Boolean(resolveSvgFromOutput());
  };

  const setControlsVisibility = (visible: boolean): void => {
    options.controlsRoot.hidden = !visible;
    options.exportToggleButton.hidden = !visible;
    if (!visible) {
      options.exportMenu.hidden = true;
    }
  };

  const fitToViewport = (): void => {
    if (!panzoom || !currentSvg || !outputRoot) {
      return;
    }

    const { width: outputWidth, height: outputHeight } =
      getViewportDimensions(outputRoot);
    if (!currentPanTargetBounds || hasUsableViewBox(currentSvg)) {
      currentPanTargetBounds = getDiagramBounds(currentSvg);
    }
    const bounds = currentPanTargetBounds;
    const width = bounds.width;
    const height = bounds.height;
    if (outputWidth <= 0 || outputHeight <= 0 || width <= 0 || height <= 0) {
      panzoom.reset();
      updateZoomLabel();
      return;
    }

    const nextScale = clamp(
      Math.min(1, outputWidth / width, outputHeight / height),
      MIN_SCALE,
      MAX_SCALE,
    );
    const centeredPan = panForCenteredView(outputWidth, outputHeight, bounds, nextScale);
    panzoom.zoom(nextScale, { animate: false, force: true });
    panzoom.pan(centeredPan.x, centeredPan.y, { animate: false, force: true });
    viewAnchor = {
      centerXRatio: 0.5,
      centerYRatio: 0.5,
      scale: nextScale,
    };
    updateZoomLabel();
  };

  const applyViewAnchor = (
    anchor: ViewAnchor,
    options?: { allowDownscale?: boolean },
  ): boolean => {
    if (!panzoom || !outputRoot || !currentPanTargetBounds) {
      return false;
    }

    const { width: outputWidth, height: outputHeight } =
      getViewportDimensions(outputRoot);
    const { minX, minY, width, height } = currentPanTargetBounds;
    if (outputWidth <= 0 || outputHeight <= 0 || width <= 0 || height <= 0) {
      return false;
    }

    const fitScale = clamp(
      Math.min(1, outputWidth / width, outputHeight / height),
      MIN_SCALE,
      MAX_SCALE,
    );
    const candidateScale = clamp(anchor.scale, MIN_SCALE, MAX_SCALE);
    const nextScale =
      options?.allowDownscale === true
        ? Math.min(candidateScale, fitScale)
        : candidateScale;
    const centerX = minX + clampRatio(anchor.centerXRatio) * width;
    const centerY = minY + clampRatio(anchor.centerYRatio) * height;
    const panX = outputWidth / (2 * nextScale) - centerX;
    const panY = outputHeight / (2 * nextScale) - centerY;
    panzoom.zoom(nextScale, { animate: false, force: true });
    panzoom.pan(panX, panY, { animate: false, force: true });
    viewAnchor = {
      centerXRatio: clampRatio(anchor.centerXRatio),
      centerYRatio: clampRatio(anchor.centerYRatio),
      scale: nextScale,
    };
    updateZoomLabel();
    return true;
  };

  const attachPanzoom = (svg: SVGSVGElement): void => {
    const panTarget: SVGElement = svg;
    if (currentSvg === svg && currentPanTarget === panTarget && panzoom) {
      updateZoomLabel();
      return;
    }

    teardownPanzoom();
    currentSvg = svg;
    currentPanTarget = panTarget;
    normalizeSvgSize(svg);
    currentPanTargetBounds = getDiagramBounds(svg);
    panzoom = dependencies.createPanzoom(panTarget);
    wheelHost = outputRoot;
    wheelHost?.addEventListener("wheel", handleWheel as EventListener, {
      passive: false,
    });
    fitToViewport();

    const currentTicket = ++fitTicket;
    const deferredFit = (): void => {
      if (currentTicket !== fitTicket || !panzoom || currentSvg !== svg) {
        return;
      }

      // Force a post-attach layout read before applying a second fit.
      // This avoids occasional SVG/foreignObject text paint glitches.
      void svg.getBoundingClientRect();
      fitToViewport();
      forceSvgRepaint();
    };

    if (
      typeof window !== "undefined" &&
      typeof window.requestAnimationFrame === "function"
    ) {
      window.requestAnimationFrame(() => {
        window.requestAnimationFrame(deferredFit);
      });
      return;
    }

    setTimeout(deferredFit, 0);
  };

  const refresh = (): void => {
    const visible = shouldShowControls();
    setControlsVisibility(visible);
    if (!visible) {
      teardownPanzoom();
      return;
    }

    const svg = resolveSvgFromOutput();
    if (!svg) {
      teardownPanzoom();
      return;
    }

    attachPanzoom(svg);
  };

  function handleWheel(event: WheelEvent): void {
    if (!panzoom) {
      return;
    }
    panzoom.zoomWithWheel(event);
    if (outputRoot && currentPanTargetBounds) {
      viewAnchor = captureViewAnchor(outputRoot, panzoom, currentPanTargetBounds);
    }
    updateZoomLabel();
    forceSvgRepaint();
  }

  const withCurrentSvg = (): SVGSVGElement | null => {
    const svg = resolveSvgFromOutput();
    if (!svg || currentFormat !== "svg") {
      reportStatus("Export is available when SVG preview is active.");
      return null;
    }
    return svg;
  };

  const triggerDownload = (blob: Blob, fileName: string): void => {
    const objectUrl = dependencies.createObjectUrl(blob);
    try {
      const anchor = dependencies.createAnchor();
      anchor.href = objectUrl;
      anchor.download = fileName;
      anchor.rel = "noopener";
      document.body.append(anchor);
      anchor.click();
      anchor.remove();
    } finally {
      dependencies.revokeObjectUrl(objectUrl);
    }
  };

  const exportSvg = (): void => {
    const svg = withCurrentSvg();
    if (!svg) {
      return;
    }

    const blob = new Blob([serializeSvg(svg)], {
      type: "image/svg+xml;charset=utf-8",
    });
    triggerDownload(blob, "mmdflux-diagram.svg");
    reportStatus("Downloaded SVG.");
  };

  const createPngBlob = async (svg: SVGSVGElement): Promise<Blob> => {
    const serialized = serializeSvg(svg);
    const { width, height } = getSvgDimensions(svg);
    const sourceBlob = new Blob([serialized], {
      type: "image/svg+xml;charset=utf-8",
    });
    const sourceUrl = dependencies.createObjectUrl(sourceBlob);

    try {
      const image = dependencies.createImage();
      await new Promise<void>((resolve, reject) => {
        image.onload = () => resolve();
        image.onerror = () =>
          reject(new Error("Could not decode SVG for PNG conversion."));
        image.src = sourceUrl;
      });

      const canvas = dependencies.createCanvas();
      const ratio = Math.max(dependencies.devicePixelRatio(), 1);
      canvas.width = Math.max(1, Math.round(width * ratio));
      canvas.height = Math.max(1, Math.round(height * ratio));
      const context = canvas.getContext("2d");
      if (!context) {
        throw new Error("Canvas context is unavailable.");
      }

      context.scale(ratio, ratio);
      context.drawImage(image, 0, 0, width, height);

      return await new Promise<Blob>((resolve, reject) => {
        canvas.toBlob((blob) => {
          if (!blob) {
            reject(new Error("Canvas export failed."));
            return;
          }
          resolve(blob);
        }, "image/png");
      });
    } finally {
      dependencies.revokeObjectUrl(sourceUrl);
    }
  };

  const exportPng = async (): Promise<void> => {
    const svg = withCurrentSvg();
    if (!svg) {
      return;
    }

    options.exportPngButton.disabled = true;
    try {
      const blob = await createPngBlob(svg);
      triggerDownload(blob, "mmdflux-diagram.png");
      reportStatus("Downloaded PNG.");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      reportStatus(`PNG export failed: ${message}`);
    } finally {
      options.exportPngButton.disabled = false;
    }
  };

  const toggleExportMenu = (): void => {
    if (options.exportToggleButton.hidden) {
      options.exportMenu.hidden = true;
      return;
    }

    options.exportMenu.hidden = !options.exportMenu.hidden;
  };

  const closeExportMenu = (): void => {
    options.exportMenu.hidden = true;
  };

  const handleDocumentClick = (event: MouseEvent): void => {
    const target = event.target;
    if (!(target instanceof Node)) {
      return;
    }
    if (
      options.exportMenu.contains(target) ||
      options.exportToggleButton.contains(target)
    ) {
      return;
    }
    closeExportMenu();
  };

  options.zoomOutButton.addEventListener("click", () => {
    if (!panzoom) {
      return;
    }

    panzoom.zoomOut({ step: ZOOM_STEP });
    if (outputRoot && currentPanTargetBounds) {
      viewAnchor = captureViewAnchor(outputRoot, panzoom, currentPanTargetBounds);
    }
    updateZoomLabel();
    forceSvgRepaint();
  });

  options.zoomInButton.addEventListener("click", () => {
    if (!panzoom) {
      return;
    }

    panzoom.zoomIn({ step: ZOOM_STEP });
    if (outputRoot && currentPanTargetBounds) {
      viewAnchor = captureViewAnchor(outputRoot, panzoom, currentPanTargetBounds);
    }
    updateZoomLabel();
    forceSvgRepaint();
  });

  options.zoomFitButton.addEventListener("click", () => {
    fitToViewport();
  });

  options.zoomResetButton.addEventListener("click", () => {
    if (!panzoom) {
      return;
    }

    if (!outputRoot || !currentPanTargetBounds) {
      panzoom.reset();
      updateZoomLabel();
      forceSvgRepaint();
      return;
    }

    const { width: outputWidth, height: outputHeight } =
      getViewportDimensions(outputRoot);
    const bounds = currentPanTargetBounds;
    const { width, height } = bounds;
    if (outputWidth <= 0 || outputHeight <= 0 || width <= 0 || height <= 0) {
      panzoom.reset();
      updateZoomLabel();
      forceSvgRepaint();
      return;
    }

    const resetScale = clamp(1, MIN_SCALE, MAX_SCALE);
    const centeredPan = panForCenteredView(
      outputWidth,
      outputHeight,
      bounds,
      resetScale,
    );
    panzoom.zoom(resetScale, { animate: false, force: true });
    panzoom.pan(centeredPan.x, centeredPan.y, { animate: false, force: true });
    viewAnchor = {
      centerXRatio: 0.5,
      centerYRatio: 0.5,
      scale: resetScale,
    };
    updateZoomLabel();
    forceSvgRepaint();
  });

  options.exportToggleButton.addEventListener("click", () => {
    toggleExportMenu();
  });

  options.exportSvgButton.addEventListener("click", () => {
    closeExportMenu();
    exportSvg();
  });

  options.exportPngButton.addEventListener("click", () => {
    closeExportMenu();
    void exportPng();
  });

  document.addEventListener("click", handleDocumentClick);

  resetZoomLabel();
  setControlsVisibility(false);

  return {
    attachTo: (nextOutputRoot) => {
      outputRoot = nextOutputRoot;
      refresh();
    },
    onResult: (format) => {
      currentFormat = format;
      refresh();
    },
    setStatusReporter: (reporter) => {
      reportStatus = reporter;
    },
    dispose: () => {
      document.removeEventListener("click", handleDocumentClick);
      teardownPanzoom();
      setControlsVisibility(false);
    },
  };
}
