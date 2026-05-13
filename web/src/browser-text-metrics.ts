export interface BrowserTextMetricsRequest {
  fontFamily?: string;
  fontSizePx?: number;
  lineHeightPx?: number;
  fontStyle?: string;
  fontWeight?: string;
  defaultStyle?: string;
  textStyles?: BrowserTextMetricsStyleRequest[];
  profileId?: string;
  profileVersion?: number;
}

export interface BrowserTextMetricsStyleRequest {
  id: string;
  fontFamily: string;
  fontSize?: number;
  fontSizePx?: number;
  lineHeight?: number;
  lineHeightPx?: number;
  fontStyle?: string;
  fontWeight?: string;
  cssFont?: string;
}

export const BROWSER_TEXT_METRICS_PROFILE_ID = "mmdflux-browser-canvas-v1";

export interface PreparedBrowserTextMetrics {
  metricsJson: string;
  measureText: (text: string, cssFont: string) => number;
}

export type BrowserTextMetricsCapabilityCode =
  | "worker-font-face-set-unavailable"
  | "worker-offscreen-canvas-unavailable"
  | "canvas-2d-context-unavailable"
  | "main-thread-font-face-set-unavailable"
  | "main-thread-canvas-unavailable"
  | "main-thread-canvas-2d-context-unavailable";

export class BrowserTextMetricsCapabilityError extends Error {
  readonly fallbackEligible: boolean;

  constructor(
    readonly code: BrowserTextMetricsCapabilityCode,
    message: string,
    fallbackEligible = true,
  ) {
    super(message);
    this.name = "BrowserTextMetricsCapabilityError";
    this.fallbackEligible = fallbackEligible;
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

export function isBrowserTextMetricsCapabilityError(
  error: unknown,
): error is BrowserTextMetricsCapabilityError {
  return error instanceof BrowserTextMetricsCapabilityError;
}

export interface BrowserTextMetricsEnvironment {
  OffscreenCanvas?: OffscreenCanvasFactory;
  fonts?: BrowserFontFaceSet;
}

export interface MainThreadBrowserTextMetricsEnvironment {
  document?: MainThreadTextMetricsDocument;
}

interface OffscreenCanvasFactory {
  new (
    width: number,
    height: number,
  ): {
    getContext(type: "2d"): CanvasTextMeasureContext | null;
  };
}

interface CanvasTextMeasureContext {
  font: string;
  measureText(text: string): { width: number };
}

interface BrowserFontFaceSet {
  load(cssFont: string): Promise<unknown[]>;
  ready?: Promise<unknown>;
  check(cssFont: string): boolean;
}

interface MainThreadTextMetricsDocument {
  fonts?: BrowserFontFaceSet;
  createElement?(tagName: "canvas"): {
    getContext(type: "2d"): CanvasTextMeasureContext | null;
  } | null;
}

export function browserTextMetricsEnvironment(
  scope: unknown = globalThis,
): BrowserTextMetricsEnvironment {
  const candidate = scope as Partial<BrowserTextMetricsEnvironment>;
  return {
    OffscreenCanvas: candidate.OffscreenCanvas,
    fonts: candidate.fonts,
  };
}

interface PreparedBrowserTextStyle {
  id: string;
  fontFamily: string;
  fontSize: number;
  fontStyle: string;
  fontWeight: string;
  lineHeight: number;
  cssFont: string;
}

interface PreparedBrowserTextStyleSet {
  defaultStyle: string;
  textStyles: PreparedBrowserTextStyle[];
  profileId: string;
  profileVersion: number;
}

export function mainThreadBrowserTextMetricsEnvironment(
  scope: unknown = globalThis,
): MainThreadBrowserTextMetricsEnvironment {
  const candidate = scope as Partial<MainThreadBrowserTextMetricsEnvironment>;
  return {
    document: candidate.document,
  };
}

export async function prepareBrowserTextMetrics(
  input: BrowserTextMetricsRequest,
  environment = browserTextMetricsEnvironment(),
): Promise<PreparedBrowserTextMetrics> {
  const styleSet = prepareTextStyleSet(input);
  const fontSet = environment.fonts;
  if (!fontSet) {
    throw new BrowserTextMetricsCapabilityError(
      "worker-font-face-set-unavailable",
      "Dynamic text metrics require worker FontFaceSet support.",
    );
  }

  const Canvas = environment.OffscreenCanvas;
  if (!Canvas) {
    throw new BrowserTextMetricsCapabilityError(
      "worker-offscreen-canvas-unavailable",
      "Dynamic text metrics require OffscreenCanvas in the worker.",
    );
  }

  const canvas = new Canvas(1, 1);
  const context = canvas.getContext("2d");
  if (!context) {
    throw new BrowserTextMetricsCapabilityError(
      "canvas-2d-context-unavailable",
      "Dynamic text metrics require a 2D canvas context.",
    );
  }

  await loadAndValidateFontSet(fontSet, styleSet.textStyles);

  return preparedMetrics(styleSet, context);
}

export async function prepareMainThreadBrowserTextMetrics(
  input: BrowserTextMetricsRequest,
  environment = mainThreadBrowserTextMetricsEnvironment(),
): Promise<PreparedBrowserTextMetrics> {
  const styleSet = prepareTextStyleSet(input);
  const document = environment.document;
  if (!document?.fonts) {
    throw new BrowserTextMetricsCapabilityError(
      "main-thread-font-face-set-unavailable",
      "Dynamic text metrics require document.fonts on the main thread.",
      false,
    );
  }

  const fontSet = document.fonts;
  if (!document.createElement) {
    throw new BrowserTextMetricsCapabilityError(
      "main-thread-canvas-unavailable",
      "Dynamic text metrics require a main-thread canvas.",
      false,
    );
  }

  const canvas = document.createElement("canvas");
  if (!canvas) {
    throw new BrowserTextMetricsCapabilityError(
      "main-thread-canvas-unavailable",
      "Dynamic text metrics require a main-thread canvas.",
      false,
    );
  }

  const context = canvas.getContext("2d");
  if (!context) {
    throw new BrowserTextMetricsCapabilityError(
      "main-thread-canvas-2d-context-unavailable",
      "Dynamic text metrics require a main-thread 2D canvas context.",
      false,
    );
  }

  await loadAndValidateFontSet(fontSet, styleSet.textStyles);

  return preparedMetrics(styleSet, context);
}

function preparedMetrics(
  styleSet: PreparedBrowserTextStyleSet,
  context: CanvasTextMeasureContext,
): PreparedBrowserTextMetrics {
  const cache = new Map<string, number>();
  return {
    metricsJson: JSON.stringify({
      defaultStyle: styleSet.defaultStyle,
      textStyles: styleSet.textStyles,
      profileId: styleSet.profileId,
      profileVersion: styleSet.profileVersion,
    }),
    measureText: (text: string, measuredCssFont: string): number => {
      const key = `${measuredCssFont}\0${text}`;
      const cached = cache.get(key);
      if (cached !== undefined) {
        return cached;
      }

      context.font = measuredCssFont;
      const width = context.measureText(text).width;
      if (!Number.isFinite(width) || width < 0) {
        throw new Error("Canvas measureText returned an invalid width.");
      }
      cache.set(key, width);
      return width;
    },
  };
}

async function loadAndValidateFontSet(
  fontSet: BrowserFontFaceSet,
  textStyles: PreparedBrowserTextStyle[],
): Promise<void> {
  // Do not await FontFaceSet.ready here. Chrome worker FontFaceSet.ready can
  // stay pending for system-font stacks even after load resolves and check
  // passes; load plus post-load check is the requested-font contract.
  for (const style of textStyles) {
    await fontSet.load(style.cssFont);
    if (!fontSet.check(style.cssFont)) {
      throw new Error(
        `Dynamic text metrics unavailable for font ${style.fontFamily}.`,
      );
    }
  }
}

export function buildCssFont(input: BrowserTextMetricsRequest): string {
  const style = legacyTextStyleInput(input);
  const fontFamily = fontFamilyStackToCss(style.fontFamily);
  validatePositiveFinite("fontSizePx", style.fontSizePx);
  validatePositiveFinite("lineHeightPx", style.lineHeightPx);

  return `${fontStyleFor(style)} ${fontWeightFor(style)} ${style.fontSizePx}px ${fontFamily}`;
}

function prepareTextStyleSet(
  input: BrowserTextMetricsRequest,
): PreparedBrowserTextStyleSet {
  if (input.textStyles && input.textStyles.length === 0) {
    throw new Error("textStyles must not be empty.");
  }
  const textStyles = input.textStyles
    ? input.textStyles.map(prepareTextStyle)
    : [prepareTextStyle({ id: "s0", ...legacyTextStyleInput(input) })];
  const defaultStyle =
    input.defaultStyle === undefined
      ? textStyles[0]?.id
      : normalizeNonEmpty("defaultStyle", input.defaultStyle);
  if (!defaultStyle) {
    throw new Error("defaultStyle must reference a textStyles id.");
  }
  if (!textStyles.some((style) => style.id === defaultStyle)) {
    throw new Error(
      `defaultStyle ${defaultStyle} must reference a textStyles id.`,
    );
  }

  const styleIds = new Set<string>();
  for (const style of textStyles) {
    if (styleIds.has(style.id)) {
      throw new Error(`textStyles contains duplicate id ${style.id}.`);
    }
    styleIds.add(style.id);
  }

  return {
    defaultStyle,
    textStyles,
    profileId: normalizedProfileId(input.profileId),
    profileVersion: normalizedProfileVersion(input.profileVersion),
  };
}

function prepareTextStyle(
  input: BrowserTextMetricsStyleRequest,
): PreparedBrowserTextStyle {
  const id = normalizeNonEmpty("textStyles.id", input.id);
  const fontFamily = normalizeFontFamily(input.fontFamily);
  const fontSize = positiveFiniteNumberWithAlias(
    "textStyles.fontSize",
    input.fontSize ?? input.fontSizePx,
    "textStyles.fontSizePx",
    input.fontSizePx,
  );
  const lineHeight = positiveFiniteNumberWithAlias(
    "textStyles.lineHeight",
    input.lineHeight ?? input.lineHeightPx,
    "textStyles.lineHeightPx",
    input.lineHeightPx,
  );
  const fontStyle = fontStyleFor(input);
  const fontWeight = fontWeightFor(input);
  const cssFont =
    input.cssFont === undefined
      ? `${fontStyle} ${fontWeight} ${fontSize}px ${fontFamilyStackToCss(fontFamily)}`
      : normalizeNonEmpty("textStyles.cssFont", input.cssFont);

  return {
    id,
    fontFamily,
    fontSize,
    fontStyle,
    fontWeight,
    lineHeight,
    cssFont,
  };
}

function legacyTextStyleInput(
  input: BrowserTextMetricsRequest,
): Required<
  Pick<BrowserTextMetricsRequest, "fontFamily" | "fontSizePx" | "lineHeightPx">
> &
  Pick<BrowserTextMetricsRequest, "fontStyle" | "fontWeight"> {
  return {
    fontFamily: input.fontFamily ?? "",
    fontSizePx: input.fontSizePx ?? Number.NaN,
    lineHeightPx: input.lineHeightPx ?? Number.NaN,
    fontStyle: input.fontStyle,
    fontWeight: input.fontWeight,
  };
}

function normalizedProfileId(profileId: string | undefined): string {
  return profileId?.trim() || BROWSER_TEXT_METRICS_PROFILE_ID;
}

function normalizedProfileVersion(profileVersion: number | undefined): number {
  if (profileVersion === undefined) {
    return 1;
  }
  if (!Number.isInteger(profileVersion) || profileVersion <= 0) {
    throw new Error("profileVersion must be a positive integer.");
  }
  return profileVersion;
}

function fontStyleFor(
  input: Pick<BrowserTextMetricsStyleRequest, "fontStyle">,
): string {
  return input.fontStyle?.trim() || "normal";
}

function fontWeightFor(
  input: Pick<BrowserTextMetricsStyleRequest, "fontWeight">,
): string {
  return input.fontWeight?.trim() || "400";
}

function normalizeFontFamily(fontFamily: string): string {
  return normalizeNonEmpty("fontFamily", fontFamily);
}

function normalizeNonEmpty(field: string, value: string): string {
  const normalized = value.trim();
  if (!normalized) {
    throw new Error(`${field} must not be empty.`);
  }
  return normalized;
}

function fontFamilyStackToCss(fontFamily: string): string {
  return normalizeFontFamily(fontFamily)
    .split(",")
    .map((family) => familyTokenToCss(family))
    .join(", ");
}

function familyTokenToCss(family: string): string {
  const unquoted = stripOneQuoteLayer(family.trim());
  if (!unquoted) {
    throw new Error("fontFamily must not contain empty family names.");
  }

  if (isGenericFamily(unquoted)) {
    return unquoted.toLowerCase();
  }

  const escaped = unquoted.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
  return `"${escaped}"`;
}

function stripOneQuoteLayer(value: string): string {
  if (
    value.length >= 2 &&
    ((value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'")))
  ) {
    return value.slice(1, -1).trim();
  }

  return value;
}

function isGenericFamily(family: string): boolean {
  switch (family.toLowerCase()) {
    case "serif":
    case "sans-serif":
    case "monospace":
    case "cursive":
    case "fantasy":
    case "system-ui":
    case "ui-serif":
    case "ui-sans-serif":
    case "ui-monospace":
    case "ui-rounded":
    case "emoji":
    case "math":
    case "fangsong":
      return true;
    default:
      return false;
  }
}

function validatePositiveFinite(field: string, value: number): void {
  if (!Number.isFinite(value) || value <= 0) {
    throw new Error(`${field} must be a finite positive number.`);
  }
}

function positiveFiniteNumber(field: string, value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) {
    throw new Error(`${field} must be a finite positive number.`);
  }
  return value;
}

function positiveFiniteNumberWithAlias(
  field: string,
  value: unknown,
  aliasField: string,
  aliasValue: unknown,
): number {
  const primary = positiveFiniteNumber(field, value);
  if (aliasValue === undefined) {
    return primary;
  }

  const alias = positiveFiniteNumber(aliasField, aliasValue);
  if (primary !== alias) {
    throw new Error(
      `${field} and ${aliasField} must match when both are provided.`,
    );
  }
  return primary;
}
