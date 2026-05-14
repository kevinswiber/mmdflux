import { MmdsBrowserTextMetricsCapabilityError } from "./capability.js";
import { buildCssFont } from "./css-font.js";
import {
  MMDS_BROWSER_TEXT_METRICS_PROFILE_ID,
  MMDS_BROWSER_TEXT_METRICS_PROFILE_VERSION,
} from "./profile.js";

export {
  MMDS_BROWSER_TEXT_METRICS_PROFILE_ID,
  MMDS_BROWSER_TEXT_METRICS_PROFILE_VERSION,
};

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

export interface PreparedBrowserTextMetrics {
  readonly metricsJson: string;
  readonly measureText: (text: string, cssFont: string) => number;
}

export interface BrowserTextMetricsEnvironment {
  OffscreenCanvas?: OffscreenCanvasFactory;
  fonts?: BrowserFontFaceSet;
}

export interface MainThreadBrowserTextMetricsEnvironment {
  document?: MainThreadTextMetricsDocument;
}

export interface PrepareWorkerTextMetricsOptions {
  readonly request: BrowserTextMetricsRequest;
  readonly environment?: BrowserTextMetricsEnvironment;
}

export interface PrepareMainThreadTextMetricsOptions {
  readonly request: BrowserTextMetricsRequest;
  readonly environment?: MainThreadBrowserTextMetricsEnvironment;
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

export function browserTextMetricsEnvironment(
  scope: unknown = globalThis,
): BrowserTextMetricsEnvironment {
  const candidate = scope as Partial<BrowserTextMetricsEnvironment>;
  return {
    OffscreenCanvas: candidate.OffscreenCanvas,
    fonts: candidate.fonts,
  };
}

export function mainThreadBrowserTextMetricsEnvironment(
  scope: unknown = globalThis,
): MainThreadBrowserTextMetricsEnvironment {
  const candidate = scope as Partial<MainThreadBrowserTextMetricsEnvironment>;
  return { document: candidate.document };
}

export async function prepareWorkerTextMetrics(
  options: PrepareWorkerTextMetricsOptions,
): Promise<PreparedBrowserTextMetrics> {
  const environment = options.environment ?? browserTextMetricsEnvironment();
  const styleSet = prepareTextStyleSet(options.request);
  const fontSet = environment.fonts;
  if (!fontSet) {
    throw new MmdsBrowserTextMetricsCapabilityError({
      code: "worker-font-face-set-unavailable",
      message: "Dynamic text metrics require worker FontFaceSet support.",
    });
  }

  const Canvas = environment.OffscreenCanvas;
  if (!Canvas) {
    throw new MmdsBrowserTextMetricsCapabilityError({
      code: "worker-offscreen-canvas-unavailable",
      message: "Dynamic text metrics require OffscreenCanvas in the worker.",
    });
  }

  const canvas = new Canvas(1, 1);
  const context = canvas.getContext("2d");
  if (!context) {
    throw new MmdsBrowserTextMetricsCapabilityError({
      code: "worker-canvas-2d-context-unavailable",
      message: "Dynamic text metrics require a 2D canvas context.",
    });
  }

  await loadAndValidateFontSet(fontSet, styleSet.textStyles);
  return preparedMetrics(styleSet, context);
}

export async function prepareMainThreadTextMetrics(
  options: PrepareMainThreadTextMetricsOptions,
): Promise<PreparedBrowserTextMetrics> {
  const environment =
    options.environment ?? mainThreadBrowserTextMetricsEnvironment();
  const styleSet = prepareTextStyleSet(options.request);
  const document = environment.document;
  if (!document?.fonts) {
    throw new MmdsBrowserTextMetricsCapabilityError({
      code: "main-thread-font-face-set-unavailable",
      message:
        "Dynamic text metrics require document.fonts on the main thread.",
    });
  }

  const fontSet = document.fonts;
  if (!document.createElement) {
    throw new MmdsBrowserTextMetricsCapabilityError({
      code: "main-thread-canvas-unavailable",
      message: "Dynamic text metrics require a main-thread canvas.",
    });
  }

  const canvas = document.createElement("canvas");
  if (!canvas) {
    throw new MmdsBrowserTextMetricsCapabilityError({
      code: "main-thread-canvas-unavailable",
      message: "Dynamic text metrics require a main-thread canvas.",
    });
  }

  const context = canvas.getContext("2d");
  if (!context) {
    throw new MmdsBrowserTextMetricsCapabilityError({
      code: "main-thread-canvas-2d-context-unavailable",
      message: "Dynamic text metrics require a main-thread 2D canvas context.",
    });
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
        throw new MmdsBrowserTextMetricsCapabilityError({
          code: "font-load-check-failed",
          message: "Canvas measureText returned an invalid width.",
          fallbackEligible: false,
          cssFont: measuredCssFont,
        });
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
      throw new MmdsBrowserTextMetricsCapabilityError({
        code: "font-load-check-failed",
        message: `Dynamic text metrics unavailable for font ${style.fontFamily}.`,
        fallbackEligible: false,
        cssFont: style.cssFont,
      });
    }
  }
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
  // textStyles is guaranteed non-empty here: the input.textStyles path
  // throws above for length 0, and the legacy fallback emits exactly one
  // entry. The non-null assertion documents that invariant for the type
  // checker without adding a runtime branch the array length already rules out.
  const defaultStyle =
    input.defaultStyle === undefined
      ? textStyles[0].id
      : normalizeNonEmpty("defaultStyle", input.defaultStyle);
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
  const fontFamily = normalizeNonEmpty("fontFamily", input.fontFamily);
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
      ? buildCssFont({
          fontFamily,
          fontSizePx: fontSize,
          lineHeightPx: lineHeight,
          fontStyle,
          fontWeight,
        })
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
  return profileId?.trim() || MMDS_BROWSER_TEXT_METRICS_PROFILE_ID;
}

function normalizedProfileVersion(profileVersion: number | undefined): number {
  if (profileVersion === undefined) {
    return MMDS_BROWSER_TEXT_METRICS_PROFILE_VERSION;
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

function normalizeNonEmpty(field: string, value: string): string {
  const normalized = value.trim();
  if (!normalized) {
    throw new Error(`${field} must not be empty.`);
  }
  return normalized;
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
  if (aliasValue === undefined) return primary;
  const alias = positiveFiniteNumber(aliasField, aliasValue);
  if (primary !== alias) {
    throw new Error(
      `${field} and ${aliasField} must match when both are provided.`,
    );
  }
  return primary;
}
