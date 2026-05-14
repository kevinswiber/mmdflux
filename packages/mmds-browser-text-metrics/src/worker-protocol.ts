export const PROTOCOL_VERSION = 1 as const;
export type WorkerProtocolVersion = typeof PROTOCOL_VERSION;

export type WorkerOutputFormat = "text" | "ascii" | "svg" | "mmds" | "mermaid";

// Minimal wire shape consumed by the worker handler. The full preflight
// surface (with profile id, version, textStyles, etc.) lives in prepare.ts
// and is structurally compatible.
export interface WorkerBrowserTextMetricsRequest {
  fontFamily?: string;
  fontSizePx?: number;
  lineHeightPx?: number;
  fontStyle?: string;
  fontWeight?: string;
  defaultStyle?: string;
  textStyles?: Array<{
    id: string;
    fontFamily: string;
    fontSize?: number;
    fontSizePx?: number;
    lineHeight?: number;
    lineHeightPx?: number;
    fontStyle?: string;
    fontWeight?: string;
    cssFont?: string;
  }>;
  profileId?: string;
  profileVersion?: number;
}

export interface WorkerBrowserTextMetricsDecision {
  required: boolean;
  browserTextMetrics?: WorkerBrowserTextMetricsRequest;
}

export interface WorkerRenderRequestMessage {
  version: WorkerProtocolVersion;
  type: "render";
  seq: number;
  input: string;
  format: WorkerOutputFormat;
  configJson: string;
}

export interface WorkerValidateRequestMessage {
  version: WorkerProtocolVersion;
  type: "validate";
  seq: number;
  input: string;
}

export interface WorkerBrowserTextMetricsRequestMessage {
  version: WorkerProtocolVersion;
  type: "resolveBrowserTextMetrics";
  seq: number;
  input: string;
  format: WorkerOutputFormat;
  configJson: string;
}

export interface WorkerDynamicTextMetricsRenderRequestMessage {
  version: WorkerProtocolVersion;
  type: "renderWithBrowserTextMetrics";
  seq: number;
  input: string;
  format: "svg";
  configJson: string;
  browserTextMetrics: WorkerBrowserTextMetricsRequest;
}

export type WorkerRequestMessage =
  | WorkerRenderRequestMessage
  | WorkerValidateRequestMessage
  | WorkerBrowserTextMetricsRequestMessage
  | WorkerDynamicTextMetricsRenderRequestMessage;

export interface WorkerResultMessage {
  version: WorkerProtocolVersion;
  type: "result";
  seq: number;
  format: WorkerOutputFormat;
  output: string;
}

export interface WorkerValidationMessage {
  version: WorkerProtocolVersion;
  type: "validation";
  seq: number;
  resultJson: string;
}

export interface WorkerBrowserTextMetricsDecisionMessage {
  version: WorkerProtocolVersion;
  type: "browserTextMetricsDecision";
  seq: number;
  decision: WorkerBrowserTextMetricsDecision;
}

export interface WorkerErrorMessage {
  version: WorkerProtocolVersion;
  type: "error";
  seq: number;
  error: string;
  code?:
    | "dynamic-metrics-capability"
    | "unsupported-format"
    | "wasm-render-rejected"
    | "wasm-config-rejected"
    | "wasm-reentered";
}

export type WorkerResponseMessage =
  | WorkerResultMessage
  | WorkerValidationMessage
  | WorkerBrowserTextMetricsDecisionMessage
  | WorkerErrorMessage;

const RENDER_FORMATS: ReadonlySet<string> = Object.freeze(
  new Set<WorkerOutputFormat>(["text", "ascii", "svg", "mmds", "mermaid"]),
);

const TEXT_STYLE_NUMBER_FIELDS = [
  "fontSize",
  "fontSizePx",
  "lineHeight",
  "lineHeightPx",
] as const;
const TEXT_STYLE_STRING_FIELDS = [
  "fontFamily",
  "fontStyle",
  "fontWeight",
  "cssFont",
] as const;

function isObjectWithBrowserTextMetrics(value: unknown): boolean {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  if (v.fontFamily !== undefined && typeof v.fontFamily !== "string") {
    return false;
  }
  if (v.fontSizePx !== undefined && typeof v.fontSizePx !== "number") {
    return false;
  }
  if (v.lineHeightPx !== undefined && typeof v.lineHeightPx !== "number") {
    return false;
  }
  if (v.textStyles !== undefined) {
    if (!Array.isArray(v.textStyles)) return false;
    for (const style of v.textStyles) {
      if (typeof style !== "object" || style === null) return false;
      const s = style as Record<string, unknown>;
      if (typeof s.id !== "string") return false;
      // fontFamily is required on the wire (used by preflight to build cssFont).
      if (typeof s.fontFamily !== "string") return false;
      for (const field of TEXT_STYLE_NUMBER_FIELDS) {
        if (s[field] !== undefined && typeof s[field] !== "number") {
          return false;
        }
      }
      for (const field of TEXT_STYLE_STRING_FIELDS) {
        if (s[field] !== undefined && typeof s[field] !== "string") {
          return false;
        }
      }
    }
  }
  return true;
}

/**
 * Validate the JSON-decoded shape of `browserTextMetricsRequest`'s return.
 * The wasm crate is trusted, but pinning the wire contract here means
 * future drift surfaces as a clear package-level error rather than an
 * uncoded generic failure deeper in the orchestrator. Mirrors
 * `assertMmdsWasmExports` for the export surface.
 */
export function isWorkerBrowserTextMetricsDecision(
  value: unknown,
): value is WorkerBrowserTextMetricsDecision {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  if (typeof v.required !== "boolean") return false;
  if (v.browserTextMetrics !== undefined) {
    if (!isObjectWithBrowserTextMetrics(v.browserTextMetrics)) return false;
  }
  return true;
}

/**
 * Discriminant-aware structural guard. The handler entry point relies on
 * this to narrow `unknown` postMessage payloads to a concrete request
 * variant, so each branch validates the fields its handler will read —
 * an envelope-only check would force the handler to re-validate everything.
 */
export function isWorkerRequestMessage(
  value: unknown,
): value is WorkerRequestMessage {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  if (v.version !== PROTOCOL_VERSION) return false;
  if (typeof v.seq !== "number") return false;
  switch (v.type) {
    case "render":
      return (
        typeof v.input === "string" &&
        typeof v.format === "string" &&
        RENDER_FORMATS.has(v.format) &&
        typeof v.configJson === "string"
      );
    case "validate":
      return typeof v.input === "string";
    case "resolveBrowserTextMetrics":
      return (
        typeof v.input === "string" &&
        typeof v.format === "string" &&
        RENDER_FORMATS.has(v.format) &&
        typeof v.configJson === "string"
      );
    case "renderWithBrowserTextMetrics":
      return (
        typeof v.input === "string" &&
        v.format === "svg" &&
        typeof v.configJson === "string" &&
        isObjectWithBrowserTextMetrics(v.browserTextMetrics)
      );
    default:
      return false;
  }
}
