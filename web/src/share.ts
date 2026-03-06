export type ShareFormat = "text" | "svg" | "mmds";
export type ShareLayoutEngine = "auto" | "flux-layered" | "mermaid-layered";
export type ShareEdgePreset =
  | "auto"
  | "straight"
  | "step"
  | "smooth-step"
  | "curved-step"
  | "basis";
export type ShareGeometryLevel = "layout" | "routed";
export type SharePathSimplification = "none" | "lossless" | "lossy" | "minimal";
export type ShareTextPreviewMode = "plain" | "styled" | "ansi";
type LegacySharePathDetail = "full" | "compact" | "simplified" | "endpoints";

export interface ShareRenderSettings {
  layoutEngine: ShareLayoutEngine;
  edgePreset: ShareEdgePreset;
  geometryLevel: ShareGeometryLevel;
  pathSimplification: SharePathSimplification;
}

export const DEFAULT_SHARE_RENDER_SETTINGS: ShareRenderSettings = {
  layoutEngine: "auto",
  edgePreset: "auto",
  geometryLevel: "layout",
  pathSimplification: "lossless",
};

export interface ShareState {
  input: string;
  format: ShareFormat;
  textPreviewMode: ShareTextPreviewMode;
  renderSettings: ShareRenderSettings;
}

interface ShareWireStateV1 {
  v: 1;
  input: string;
  format: ShareFormat;
}

interface ShareWireStateV2 {
  v: 2;
  input: string;
  format: ShareFormat;
  renderSettings?: Partial<ShareRenderSettings>;
}

interface ShareWireStateV3 {
  v: 3;
  input: string;
  format: ShareFormat;
  textPreviewMode?: ShareTextPreviewMode | "escapes";
  renderSettings?: Partial<ShareRenderSettings>;
}

type ShareWireState = ShareWireStateV1 | ShareWireStateV2 | ShareWireStateV3;

function isShareFormat(value: string): value is ShareFormat {
  return value === "text" || value === "svg" || value === "mmds";
}

function isLayoutEngine(value: string): value is ShareLayoutEngine {
  return (
    value === "auto" || value === "flux-layered" || value === "mermaid-layered"
  );
}

function isEdgePreset(value: string): value is ShareEdgePreset {
  return (
    value === "auto" ||
    value === "straight" ||
    value === "step" ||
    value === "smooth-step" ||
    value === "curved-step" ||
    value === "basis"
  );
}

function normalizeEdgePreset(value: string): ShareEdgePreset | null {
  if (isEdgePreset(value)) {
    return value;
  }
  if (value === "smoothstep") {
    return "smooth-step";
  }
  if (value === "curvedstep") {
    return "curved-step";
  }
  return null;
}

function isGeometryLevel(value: string): value is ShareGeometryLevel {
  return value === "layout" || value === "routed";
}

function isPathSimplification(value: string): value is SharePathSimplification {
  return (
    value === "none" ||
    value === "lossless" ||
    value === "lossy" ||
    value === "minimal"
  );
}

function isTextPreviewMode(value: string): value is ShareTextPreviewMode {
  return value === "plain" || value === "styled" || value === "ansi";
}

export function normalizeShareTextPreviewMode(
  value: unknown,
): ShareTextPreviewMode {
  if (value === "escapes") {
    return "ansi";
  }

  return typeof value === "string" && isTextPreviewMode(value)
    ? value
    : "plain";
}

function isLegacyPathDetail(value: string): value is LegacySharePathDetail {
  return (
    value === "full" ||
    value === "compact" ||
    value === "simplified" ||
    value === "endpoints"
  );
}

function pathSimplificationFromLegacyPathDetail(
  detail: LegacySharePathDetail,
): SharePathSimplification {
  switch (detail) {
    case "full":
      return "none";
    case "compact":
      return "lossless";
    case "simplified":
      return "lossy";
    case "endpoints":
      return "minimal";
  }
}

export function normalizeShareRenderSettings(
  value: unknown,
): ShareRenderSettings {
  const settings =
    typeof value === "object" && value !== null
      ? (value as Partial<ShareRenderSettings> & {
          pathDetail?: string;
        })
      : {};

  const layoutEngine =
    typeof settings.layoutEngine === "string" &&
    isLayoutEngine(settings.layoutEngine)
      ? settings.layoutEngine
      : DEFAULT_SHARE_RENDER_SETTINGS.layoutEngine;
  const edgePreset =
    typeof settings.edgePreset === "string"
      ? (normalizeEdgePreset(settings.edgePreset) ??
        DEFAULT_SHARE_RENDER_SETTINGS.edgePreset)
      : DEFAULT_SHARE_RENDER_SETTINGS.edgePreset;
  const geometryLevel =
    typeof settings.geometryLevel === "string" &&
    isGeometryLevel(settings.geometryLevel)
      ? settings.geometryLevel
      : DEFAULT_SHARE_RENDER_SETTINGS.geometryLevel;
  const pathSimplification =
    typeof settings.pathSimplification === "string" &&
    isPathSimplification(settings.pathSimplification)
      ? settings.pathSimplification
      : typeof settings.pathDetail === "string" &&
          isLegacyPathDetail(settings.pathDetail)
        ? pathSimplificationFromLegacyPathDetail(settings.pathDetail)
        : DEFAULT_SHARE_RENDER_SETTINGS.pathSimplification;

  return {
    layoutEngine,
    edgePreset,
    geometryLevel,
    pathSimplification,
  };
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary);
}

function base64ToBytes(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

function base64UrlEncode(value: string): string {
  const encoded = bytesToBase64(new TextEncoder().encode(value));
  return encoded.replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", "");
}

function base64UrlDecode(value: string): string {
  const normalized = value
    .replaceAll("-", "+")
    .replaceAll("_", "/")
    .padEnd(Math.ceil(value.length / 4) * 4, "=");
  const bytes = base64ToBytes(normalized);
  return new TextDecoder().decode(bytes);
}

export function encodeShareState(state: ShareState): string {
  const wireState: ShareWireStateV3 = {
    v: 3,
    input: state.input,
    format: state.format,
    textPreviewMode: state.textPreviewMode,
    renderSettings: state.renderSettings,
  };
  return base64UrlEncode(JSON.stringify(wireState));
}

export function decodeShareState(value: string): ShareState | null {
  const normalized = value.startsWith("#") ? value.slice(1) : value;
  if (!normalized) {
    return null;
  }

  try {
    const decoded = JSON.parse(
      base64UrlDecode(normalized),
    ) as Partial<ShareWireState>;
    if (decoded.v !== 1 && decoded.v !== 2 && decoded.v !== 3) {
      return null;
    }
    if (typeof decoded.input !== "string") {
      return null;
    }
    if (typeof decoded.format !== "string" || !isShareFormat(decoded.format)) {
      return null;
    }

    return {
      input: decoded.input,
      format: decoded.format,
      textPreviewMode:
        decoded.v === 3
          ? normalizeShareTextPreviewMode(decoded.textPreviewMode)
          : "plain",
      renderSettings:
        decoded.v === 2 || decoded.v === 3
          ? normalizeShareRenderSettings(decoded.renderSettings)
          : DEFAULT_SHARE_RENDER_SETTINGS,
    };
  } catch {
    return null;
  }
}
