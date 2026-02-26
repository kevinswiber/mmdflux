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
type PanzoomChangeEvent = Event;

interface ViewAnchor {
  panX: number;
  panY: number;
  scale: number;
}

interface DiagramBounds {
  minX: number;
  minY: number;
  width: number;
  height: number;
}

interface PreviewControlDependencies {
  createPanzoom: (
    target: SVGElement,
    initialState?: ViewAnchor,
  ) => PanzoomInstance;
  createObjectUrl: (blob: Blob) => string;
  revokeObjectUrl: (url: string) => void;
  createAnchor: () => HTMLAnchorElement;
  createImage: () => HTMLImageElement;
  createCanvas: () => HTMLCanvasElement;
  devicePixelRatio: () => number;
}

interface CreatePreviewControlsOptions {
  viewportRoot?: HTMLElement;
  controlsOverlayRoot: HTMLElement;
  controlsToggleButton: HTMLButtonElement;
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
  fitOnNextSvg: () => void;
  onResult: (format: WorkerOutputFormat) => void;
  setStatusReporter: (reporter: StatusReporter) => void;
  dispose: () => void;
}

const MIN_SCALE = 0.2;
const MAX_SCALE = 20;
const ZOOM_STEP = 0.2;
const CONTENT_BOUNDS_PADDING_PX = 8;
const MIN_USABLE_BBOX_SIZE = 2;

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

function getGraphicsBounds(target: SVGGraphicsElement): DiagramBounds | null {
  try {
    const bounds = toFiniteBounds(target.getBBox());
    if (!bounds) {
      return null;
    }
    if (
      bounds.width < MIN_USABLE_BBOX_SIZE ||
      bounds.height < MIN_USABLE_BBOX_SIZE
    ) {
      return null;
    }
    return bounds;
  } catch {
    return null;
  }
}

function getDiagramBounds(
  svg: SVGSVGElement,
  panTarget: SVGGraphicsElement,
): DiagramBounds {
  const mapUserRectToCssPixels = (
    userBounds: DiagramBounds,
  ): DiagramBounds | null => {
    const matrix =
      typeof svg.getScreenCTM === "function" ? svg.getScreenCTM() : null;
    const svgRect = svg.getBoundingClientRect();
    if (
      !matrix ||
      !Number.isFinite(svgRect.left) ||
      !Number.isFinite(svgRect.top)
    ) {
      return null;
    }

    const corners = [
      new DOMPoint(userBounds.minX, userBounds.minY).matrixTransform(matrix),
      new DOMPoint(
        userBounds.minX + userBounds.width,
        userBounds.minY,
      ).matrixTransform(matrix),
      new DOMPoint(
        userBounds.minX,
        userBounds.minY + userBounds.height,
      ).matrixTransform(matrix),
      new DOMPoint(
        userBounds.minX + userBounds.width,
        userBounds.minY + userBounds.height,
      ).matrixTransform(matrix),
    ];

    if (
      corners.some(
        (point) => !Number.isFinite(point.x) || !Number.isFinite(point.y),
      )
    ) {
      return null;
    }

    const minX = Math.min(...corners.map((point) => point.x)) - svgRect.left;
    const minY = Math.min(...corners.map((point) => point.y)) - svgRect.top;
    const maxX = Math.max(...corners.map((point) => point.x)) - svgRect.left;
    const maxY = Math.max(...corners.map((point) => point.y)) - svgRect.top;
    return {
      minX,
      minY,
      width: Math.max(maxX - minX, 1),
      height: Math.max(maxY - minY, 1),
    };
  };

  const toCssPixelBounds = (userBounds: DiagramBounds): DiagramBounds => {
    if (!hasUsableViewBox(svg)) {
      return userBounds;
    }

    const mappedBounds = mapUserRectToCssPixels(userBounds);
    if (mappedBounds) {
      return mappedBounds;
    }

    const svgRect = svg.getBoundingClientRect();
    if (
      !Number.isFinite(svgRect.width) ||
      !Number.isFinite(svgRect.height) ||
      svgRect.width <= 0 ||
      svgRect.height <= 0
    ) {
      return userBounds;
    }

    const svgUserBounds = getSvgBounds(svg);
    if (svgUserBounds.width <= 0 || svgUserBounds.height <= 0) {
      return userBounds;
    }

    const scaleX = svgRect.width / svgUserBounds.width;
    const scaleY = svgRect.height / svgUserBounds.height;
    return {
      minX: (userBounds.minX - svgUserBounds.minX) * scaleX,
      minY: (userBounds.minY - svgUserBounds.minY) * scaleY,
      width: userBounds.width * scaleX,
      height: userBounds.height * scaleY,
    };
  };

  const contentBounds = getGraphicsBounds(panTarget);
  if (!contentBounds) {
    return toCssPixelBounds(getSvgBounds(svg));
  }

  const cssBounds = toCssPixelBounds(contentBounds);
  const padding = CONTENT_BOUNDS_PADDING_PX;
  return {
    minX: cssBounds.minX - padding,
    minY: cssBounds.minY - padding,
    width: Math.max(cssBounds.width + padding * 2, 1),
    height: Math.max(cssBounds.height + padding * 2, 1),
  };
}

function isStructuralSvgElement(element: Element): boolean {
  const tag = element.tagName.toLowerCase();
  return (
    tag === "defs" ||
    tag === "style" ||
    tag === "title" ||
    tag === "desc" ||
    tag === "metadata" ||
    tag === "script"
  );
}

function ensurePanzoomViewport(svg: SVGSVGElement): SVGGraphicsElement | null {
  for (const child of Array.from(svg.children)) {
    if (
      child instanceof SVGGraphicsElement &&
      child.getAttribute("data-panzoom-viewport") === "true"
    ) {
      return child;
    }
  }

  const viewport = document.createElementNS("http://www.w3.org/2000/svg", "g");
  viewport.setAttribute("data-panzoom-viewport", "true");

  let movedChildren = 0;
  for (const child of Array.from(svg.children)) {
    if (isStructuralSvgElement(child)) {
      continue;
    }

    viewport.appendChild(child);
    movedChildren += 1;
  }

  if (movedChildren > 0) {
    svg.appendChild(viewport);
    return viewport;
  }

  return svg.querySelector<SVGGraphicsElement>("g");
}

function getViewportDimensions(element: HTMLElement): {
  width: number;
  height: number;
} {
  const style = window.getComputedStyle(element);
  const paddingX =
    readCssPixels(style.paddingLeft) + readCssPixels(style.paddingRight);
  const paddingY =
    readCssPixels(style.paddingTop) + readCssPixels(style.paddingBottom);

  return {
    width: Math.max(0, element.clientWidth - paddingX),
    height: Math.max(0, element.clientHeight - paddingY),
  };
}

function captureViewAnchor(panzoom: PanzoomInstance): ViewAnchor | null {
  const scale = panzoom.getScale();
  if (!Number.isFinite(scale) || scale <= 0) {
    return null;
  }

  const pan = panzoom.getPan();
  return {
    panX: pan.x,
    panY: pan.y,
    scale,
  };
}

function normalizeSvgSize(svg: SVGSVGElement): DiagramBounds {
  const bounds = getSvgBounds(svg);
  svg.setAttribute("width", "100%");
  svg.setAttribute("height", "100%");
  svg.style.width = "100%";
  svg.style.height = "100%";
  svg.style.maxWidth = "100%";
  svg.style.maxHeight = "100%";
  svg.style.overflow = "visible";
  return bounds;
}

function panForCenteredView(
  viewportWidth: number,
  viewportHeight: number,
  bounds: DiagramBounds,
  scale: number,
  topInset = 0,
): { x: number; y: number } {
  const centerX = bounds.minX + bounds.width / 2;
  const centerY = bounds.minY + bounds.height / 2;
  const clampedTopInset = Math.max(0, Math.min(topInset, viewportHeight));
  const targetCenterY =
    clampedTopInset + (viewportHeight - clampedTopInset) / 2;
  return {
    x: viewportWidth / (2 * scale) - centerX,
    y: targetCenterY / scale - centerY,
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

function resolveSvgPanUnitsPerCssPixel(target: SVGElement): {
  x: number;
  y: number;
} {
  const ownerSvg = target.ownerSVGElement;
  if (!ownerSvg) {
    return { x: 1, y: 1 };
  }

  const svgBounds = getSvgBounds(ownerSvg);
  const ctm =
    typeof ownerSvg.getScreenCTM === "function"
      ? ownerSvg.getScreenCTM()
      : null;
  if (ctm) {
    const origin = new DOMPoint(svgBounds.minX, svgBounds.minY).matrixTransform(
      ctm,
    );
    const xStep = new DOMPoint(
      svgBounds.minX + 1,
      svgBounds.minY,
    ).matrixTransform(ctm);
    const yStep = new DOMPoint(
      svgBounds.minX,
      svgBounds.minY + 1,
    ).matrixTransform(ctm);
    const cssPixelsPerSvgUnitX = Math.hypot(
      xStep.x - origin.x,
      xStep.y - origin.y,
    );
    const cssPixelsPerSvgUnitY = Math.hypot(
      yStep.x - origin.x,
      yStep.y - origin.y,
    );
    if (
      Number.isFinite(cssPixelsPerSvgUnitX) &&
      Number.isFinite(cssPixelsPerSvgUnitY) &&
      cssPixelsPerSvgUnitX > 0 &&
      cssPixelsPerSvgUnitY > 0
    ) {
      return {
        x: 1 / cssPixelsPerSvgUnitX,
        y: 1 / cssPixelsPerSvgUnitY,
      };
    }
  }

  const svgRect = ownerSvg.getBoundingClientRect();
  if (
    !Number.isFinite(svgRect.width) ||
    !Number.isFinite(svgRect.height) ||
    svgRect.width <= 0 ||
    svgRect.height <= 0
  ) {
    return { x: 1, y: 1 };
  }

  if (svgBounds.width <= 0 || svgBounds.height <= 0) {
    return { x: 1, y: 1 };
  }

  const x = svgBounds.width / svgRect.width;
  const y = svgBounds.height / svgRect.height;
  if (!Number.isFinite(x) || !Number.isFinite(y) || x <= 0 || y <= 0) {
    return { x: 1, y: 1 };
  }

  return { x, y };
}

function defaultDependencies(): PreviewControlDependencies {
  return {
    createPanzoom: (target, initialState) =>
      Panzoom(target, {
        canvas: true,
        cursor: "",
        maxScale: MAX_SCALE,
        minScale: MIN_SCALE,
        origin: "0 0",
        roundPixels: false,
        setTransform: (elem, { scale, x, y, isSVG }) => {
          let translatedX = x;
          let translatedY = y;
          if (isSVG && elem instanceof SVGElement) {
            const units = resolveSvgPanUnitsPerCssPixel(elem);
            translatedX = x * units.x;
            translatedY = y * units.y;
          }
          elem.style.transform = `scale(${scale}) translate(${translatedX}px, ${translatedY}px)`;
        },
        startScale: initialState?.scale ?? 1,
        startX: initialState?.panX ?? 0,
        startY: initialState?.panY ?? 0,
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
  let currentPanTarget: SVGGraphicsElement | null = null;
  let currentPanTargetBounds: DiagramBounds | null = null;
  let panzoom: PanzoomInstance | null = null;
  let wheelHost: HTMLElement | null = null;
  let fitTicket = 0;
  let fitOnNextSvg = false;
  let viewAnchor: ViewAnchor | null = null;
  let controlsExpanded = false;
  let controlsVisible = false;
  let draggingActive = false;

  const updateDragCursorState = (): void => {
    if (!outputRoot) {
      return;
    }

    const draggable =
      controlsVisible &&
      currentFormat === "svg" &&
      Boolean(currentSvg) &&
      Boolean(currentPanTarget) &&
      Boolean(panzoom);
    outputRoot.classList.toggle("is-draggable", draggable);
    outputRoot.classList.toggle("is-dragging", draggable && draggingActive);
  };

  const resetDraggingState = (): void => {
    draggingActive = false;
    updateDragCursorState();
  };

  const handleOutputPointerDown = (event: PointerEvent): void => {
    if (
      event.button !== 0 ||
      !controlsVisible ||
      currentFormat !== "svg" ||
      !currentSvg ||
      !currentPanTarget ||
      !panzoom
    ) {
      return;
    }

    draggingActive = true;
    updateDragCursorState();
  };

  const handleDocumentPointerUp = (): void => {
    if (!draggingActive) {
      return;
    }
    resetDraggingState();
  };

  const handleWindowBlur = (): void => {
    if (!draggingActive) {
      return;
    }
    resetDraggingState();
  };

  const syncAnchorFromPanzoom = (): void => {
    if (!panzoom) {
      return;
    }

    const nextAnchor = captureViewAnchor(panzoom);
    if (!nextAnchor) {
      return;
    }

    viewAnchor = nextAnchor;
    updateZoomLabel();
  };

  const handlePanzoomChange = (_event: PanzoomChangeEvent): void => {
    syncAnchorFromPanzoom();
  };

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
    if (panzoom && currentPanTarget && outputRoot?.contains(currentPanTarget)) {
      const nextAnchor = captureViewAnchor(panzoom);
      if (nextAnchor) {
        viewAnchor = nextAnchor;
      }
    }
    fitTicket += 1;
    if (currentPanTarget) {
      currentPanTarget.removeEventListener(
        "panzoomchange",
        handlePanzoomChange as EventListener,
      );
    }
    if (wheelHost) {
      wheelHost.removeEventListener("wheel", handleWheel as EventListener);
    }
    wheelHost = null;
    panzoom?.destroy();
    panzoom = null;
    currentPanTarget = null;
    currentPanTargetBounds = null;
    currentSvg = null;
    resetDraggingState();
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

  const setControlsExpanded = (expanded: boolean): void => {
    controlsExpanded = expanded;
    options.controlsRoot.hidden = !controlsVisible;
    options.controlsOverlayRoot.classList.toggle("is-expanded", expanded);
    options.controlsToggleButton.setAttribute(
      "aria-expanded",
      String(expanded),
    );
    options.controlsToggleButton.setAttribute(
      "aria-label",
      expanded ? "Hide zoom controls" : "Show zoom controls",
    );
    options.controlsToggleButton.title = expanded
      ? "Hide zoom controls"
      : "Show zoom controls";
  };

  const controlsTopInset = (): number => {
    // Keep fit/reset deterministic regardless of overlay expanded/collapsed state.
    // The toolbar floats above the viewport content and should not change anchor math.
    return 0;
  };

  const setControlsVisibility = (visible: boolean): void => {
    controlsVisible = visible;
    options.controlsOverlayRoot.hidden = !visible;
    options.exportToggleButton.hidden = !visible;
    if (!visible) {
      options.exportMenu.hidden = true;
    }
    setControlsExpanded(false);
    updateDragCursorState();
  };

  const fitToViewport = (): void => {
    if (!panzoom || !currentSvg || !outputRoot) {
      return;
    }

    const fitAnchor = computeFitAnchor();
    if (!fitAnchor) {
      panzoom.reset();
      updateZoomLabel();
      return;
    }

    applyViewAnchor(fitAnchor);
    updateZoomLabel();
  };

  const computeFitAnchor = (): ViewAnchor | null => {
    if (!currentSvg || !outputRoot || !currentPanTarget) {
      return null;
    }

    const viewportRoot = options.viewportRoot ?? outputRoot;
    const { width: outputWidth, height: outputHeight } =
      getViewportDimensions(viewportRoot);
    currentPanTargetBounds = getDiagramBounds(currentSvg, currentPanTarget);
    const bounds = currentPanTargetBounds;
    const width = bounds.width;
    const height = bounds.height;
    if (outputWidth <= 0 || outputHeight <= 0 || width <= 0 || height <= 0) {
      return null;
    }
    const topInset = controlsTopInset();
    const availableHeight = Math.max(outputHeight - topInset, 0);
    if (availableHeight <= 0) {
      return null;
    }

    const nextScale = clamp(
      Math.min(1, outputWidth / width, availableHeight / height),
      MIN_SCALE,
      MAX_SCALE,
    );
    const centeredPan = panForCenteredView(
      outputWidth,
      outputHeight,
      bounds,
      nextScale,
      topInset,
    );
    return {
      panX: centeredPan.x,
      panY: centeredPan.y,
      scale: nextScale,
    };
  };

  const applyViewAnchor = (anchor: ViewAnchor): boolean => {
    if (!panzoom) {
      return false;
    }

    const candidateScale = clamp(anchor.scale, MIN_SCALE, MAX_SCALE);
    const panX = Number.isFinite(anchor.panX) ? anchor.panX : 0;
    const panY = Number.isFinite(anchor.panY) ? anchor.panY : 0;
    panzoom.zoom(candidateScale, { animate: false, force: true });
    panzoom.pan(panX, panY, { animate: false, force: true });
    viewAnchor = {
      panX,
      panY,
      scale: candidateScale,
    };
    updateZoomLabel();
    return true;
  };

  const attachPanzoom = (svg: SVGSVGElement): void => {
    const panTarget = ensurePanzoomViewport(svg);
    if (!panTarget) {
      teardownPanzoom();
      return;
    }

    if (currentSvg === svg && currentPanTarget === panTarget && panzoom) {
      updateZoomLabel();
      updateDragCursorState();
      return;
    }

    teardownPanzoom();
    currentSvg = svg;
    currentPanTarget = panTarget;
    normalizeSvgSize(svg);
    currentPanTargetBounds = getDiagramBounds(svg, panTarget);
    const fallbackFitAnchor = computeFitAnchor();
    const shouldStabilizeFit = fitOnNextSvg || !viewAnchor;
    const nextAnchor = fitOnNextSvg
      ? (fallbackFitAnchor ?? { panX: 0, panY: 0, scale: 1 })
      : (viewAnchor ?? fallbackFitAnchor ?? { panX: 0, panY: 0, scale: 1 });
    fitOnNextSvg = false;
    panzoom = dependencies.createPanzoom(panTarget, nextAnchor);
    panTarget.addEventListener(
      "panzoomchange",
      handlePanzoomChange as EventListener,
    );
    wheelHost = outputRoot;
    wheelHost?.addEventListener("wheel", handleWheel as EventListener, {
      passive: false,
    });
    applyViewAnchor(nextAnchor);
    updateDragCursorState();

    const currentTicket = ++fitTicket;
    const deferredRepaint = (): void => {
      if (currentTicket !== fitTicket || !panzoom || currentSvg !== svg) {
        return;
      }

      // Force a post-attach layout read after transform update.
      // This avoids occasional SVG/foreignObject text paint glitches.
      void svg.getBoundingClientRect();
      if (shouldStabilizeFit) {
        const stabilizedAnchor = computeFitAnchor();
        if (stabilizedAnchor) {
          applyViewAnchor(stabilizedAnchor);
        }
      }
      forceSvgRepaint();
    };

    if (
      typeof window !== "undefined" &&
      typeof window.requestAnimationFrame === "function"
    ) {
      window.requestAnimationFrame(() => {
        window.requestAnimationFrame(deferredRepaint);
      });
      return;
    }

    setTimeout(deferredRepaint, 0);
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
    viewAnchor = captureViewAnchor(panzoom);
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

  const toggleControlPanel = (): void => {
    if (options.controlsOverlayRoot.hidden) {
      return;
    }

    setControlsExpanded(!controlsExpanded);
  };

  const collapseControlPanel = (): void => {
    if (!controlsExpanded) {
      return;
    }
    setControlsExpanded(false);
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
      // keep export menu open
    } else {
      closeExportMenu();
    }

    if (options.controlsOverlayRoot.contains(target)) {
      return;
    }
    collapseControlPanel();
  };

  const handleDocumentKeydown = (event: KeyboardEvent): void => {
    if (event.key !== "Escape") {
      return;
    }

    closeExportMenu();
    collapseControlPanel();
  };

  options.zoomOutButton.addEventListener("click", () => {
    if (!panzoom) {
      return;
    }

    panzoom.zoomOut({ step: ZOOM_STEP });
    viewAnchor = captureViewAnchor(panzoom);
    updateZoomLabel();
    forceSvgRepaint();
  });

  options.zoomInButton.addEventListener("click", () => {
    if (!panzoom) {
      return;
    }

    panzoom.zoomIn({ step: ZOOM_STEP });
    viewAnchor = captureViewAnchor(panzoom);
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

    const viewportRoot = options.viewportRoot ?? outputRoot;
    const { width: outputWidth, height: outputHeight } =
      getViewportDimensions(viewportRoot);
    const bounds = currentPanTargetBounds;
    const { width, height } = bounds;
    if (outputWidth <= 0 || outputHeight <= 0 || width <= 0 || height <= 0) {
      panzoom.reset();
      updateZoomLabel();
      forceSvgRepaint();
      return;
    }

    const resetScale = clamp(1, MIN_SCALE, MAX_SCALE);
    const topInset = controlsTopInset();
    const centeredPan = panForCenteredView(
      outputWidth,
      outputHeight,
      bounds,
      resetScale,
      topInset,
    );
    panzoom.zoom(resetScale, { animate: false, force: true });
    panzoom.pan(centeredPan.x, centeredPan.y, { animate: false, force: true });
    viewAnchor = {
      panX: centeredPan.x,
      panY: centeredPan.y,
      scale: resetScale,
    };
    updateZoomLabel();
    forceSvgRepaint();
  });

  options.controlsToggleButton.addEventListener("click", (event) => {
    event.preventDefault();
    toggleControlPanel();
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
  document.addEventListener("keydown", handleDocumentKeydown);
  document.addEventListener("pointerup", handleDocumentPointerUp);
  document.addEventListener("pointercancel", handleDocumentPointerUp);
  window.addEventListener("blur", handleWindowBlur);

  resetZoomLabel();
  setControlsExpanded(false);
  setControlsVisibility(false);

  return {
    attachTo: (nextOutputRoot) => {
      if (outputRoot && outputRoot !== nextOutputRoot) {
        outputRoot.removeEventListener(
          "pointerdown",
          handleOutputPointerDown,
          true,
        );
      }
      outputRoot = nextOutputRoot;
      outputRoot.addEventListener("pointerdown", handleOutputPointerDown, true);
      refresh();
    },
    fitOnNextSvg: () => {
      fitOnNextSvg = true;
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
      document.removeEventListener("keydown", handleDocumentKeydown);
      document.removeEventListener("pointerup", handleDocumentPointerUp);
      document.removeEventListener("pointercancel", handleDocumentPointerUp);
      window.removeEventListener("blur", handleWindowBlur);
      outputRoot?.removeEventListener(
        "pointerdown",
        handleOutputPointerDown,
        true,
      );
      teardownPanzoom();
      setControlsVisibility(false);
    },
  };
}
