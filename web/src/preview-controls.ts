import Panzoom from "@panzoom/panzoom";

import type { WorkerOutputFormat } from "./worker-protocol";

interface PanzoomInstance {
  destroy: () => void;
  getScale: () => number;
  pan: (x: number, y: number, options?: Record<string, unknown>) => void;
  reset: () => void;
  zoom: (scale: number, options?: Record<string, unknown>) => void;
  zoomIn: (options?: Record<string, unknown>) => void;
  zoomOut: (options?: Record<string, unknown>) => void;
  zoomWithWheel: (event: WheelEvent) => void;
}

type StatusReporter = (message: string) => void;

interface PreviewControlDependencies {
  createPanzoom: (target: SVGSVGElement) => PanzoomInstance;
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
const MAX_SCALE = 6;
const ZOOM_STEP = 0.2;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function readNumericAttribute(
  svg: SVGSVGElement,
  name: "width" | "height",
): number {
  const raw = svg.getAttribute(name);
  if (!raw) {
    return 0;
  }

  const parsed = Number.parseFloat(raw);
  return Number.isFinite(parsed) ? parsed : 0;
}

function getSvgDimensions(svg: SVGSVGElement): {
  width: number;
  height: number;
} {
  const viewBox = svg.viewBox.baseVal;
  if (viewBox.width > 0 && viewBox.height > 0) {
    return { width: viewBox.width, height: viewBox.height };
  }

  const width = readNumericAttribute(svg, "width");
  const height = readNumericAttribute(svg, "height");
  if (width > 0 && height > 0) {
    return { width, height };
  }

  const rect = svg.getBoundingClientRect();
  return {
    width: Math.max(rect.width, 1),
    height: Math.max(rect.height, 1),
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
        maxScale: MAX_SCALE,
        minScale: MIN_SCALE,
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
  let panzoom: PanzoomInstance | null = null;
  let wheelHost: HTMLElement | null = null;

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
    if (wheelHost) {
      wheelHost.removeEventListener("wheel", handleWheel as EventListener);
    }
    wheelHost = null;
    panzoom?.destroy();
    panzoom = null;
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

    const outputWidth = outputRoot.clientWidth;
    const outputHeight = outputRoot.clientHeight;
    const { width, height } = getSvgDimensions(currentSvg);
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
    const panX = (outputWidth - width * nextScale) / 2;
    const panY = (outputHeight - height * nextScale) / 2;
    panzoom.zoom(nextScale, { animate: false });
    panzoom.pan(panX, panY, { animate: false });
    updateZoomLabel();
  };

  const attachPanzoom = (svg: SVGSVGElement): void => {
    if (currentSvg === svg && panzoom) {
      updateZoomLabel();
      return;
    }

    teardownPanzoom();
    currentSvg = svg;
    panzoom = dependencies.createPanzoom(svg);
    wheelHost = outputRoot;
    wheelHost?.addEventListener("wheel", handleWheel as EventListener, {
      passive: false,
    });
    fitToViewport();
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
    updateZoomLabel();
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
    updateZoomLabel();
  });

  options.zoomInButton.addEventListener("click", () => {
    if (!panzoom) {
      return;
    }

    panzoom.zoomIn({ step: ZOOM_STEP });
    updateZoomLabel();
  });

  options.zoomFitButton.addEventListener("click", () => {
    fitToViewport();
  });

  options.zoomResetButton.addEventListener("click", () => {
    if (!panzoom) {
      return;
    }

    panzoom.reset();
    updateZoomLabel();
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
