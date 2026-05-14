export type MmdsBrowserTextMetricsErrorCode =
  | "worker-font-face-set-unavailable"
  | "worker-offscreen-canvas-unavailable"
  | "worker-canvas-2d-context-unavailable"
  | "main-thread-font-face-set-unavailable"
  | "main-thread-canvas-unavailable"
  | "main-thread-canvas-2d-context-unavailable"
  | "font-load-check-failed"
  | "wasm-render-rejected"
  | "wasm-config-rejected"
  | "wasm-reentered"
  | "unsupported-format"
  | "invalid-text-metrics-request"
  | "dynamic-metrics-capability";

// worker-render-timeout and worker-terminated are deliberately NOT capability
// codes. The client emits them as plain Errors because they never originate
// from worker preflight, and granting them fallbackEligible: true semantics
// would conflate client policy with worker capability.

const WORKER_PREFLIGHT_CODES: ReadonlySet<MmdsBrowserTextMetricsErrorCode> =
  Object.freeze(
    new Set<MmdsBrowserTextMetricsErrorCode>([
      "worker-font-face-set-unavailable",
      "worker-offscreen-canvas-unavailable",
      "worker-canvas-2d-context-unavailable",
      "dynamic-metrics-capability",
    ]),
  );

const ALL_CODES: ReadonlySet<string> = Object.freeze(
  new Set<MmdsBrowserTextMetricsErrorCode>([
    "worker-font-face-set-unavailable",
    "worker-offscreen-canvas-unavailable",
    "worker-canvas-2d-context-unavailable",
    "main-thread-font-face-set-unavailable",
    "main-thread-canvas-unavailable",
    "main-thread-canvas-2d-context-unavailable",
    "font-load-check-failed",
    "wasm-render-rejected",
    "wasm-config-rejected",
    "wasm-reentered",
    "unsupported-format",
    "invalid-text-metrics-request",
    "dynamic-metrics-capability",
  ]),
);

export interface MmdsBrowserTextMetricsCapabilityErrorArgs {
  code: MmdsBrowserTextMetricsErrorCode;
  message: string;
  fallbackEligible?: boolean;
  cssFont?: string;
  cause?: unknown;
}

export class MmdsBrowserTextMetricsCapabilityError extends Error {
  readonly code: MmdsBrowserTextMetricsErrorCode;
  readonly fallbackEligible: boolean;
  readonly cssFont?: string;

  constructor(args: MmdsBrowserTextMetricsCapabilityErrorArgs) {
    super(args.message);
    this.name = "MmdsBrowserTextMetricsCapabilityError";
    this.code = args.code;
    this.fallbackEligible =
      args.fallbackEligible ?? WORKER_PREFLIGHT_CODES.has(args.code);
    this.cssFont = args.cssFont;
    if (args.cause !== undefined) {
      (this as { cause?: unknown }).cause = args.cause;
    }
    Object.setPrototypeOf(this, new.target.prototype);
  }

  toJSON(): {
    name: string;
    code: MmdsBrowserTextMetricsErrorCode;
    message: string;
    fallbackEligible: boolean;
    cssFont?: string;
  } {
    return {
      name: this.name,
      code: this.code,
      message: this.message,
      fallbackEligible: this.fallbackEligible,
      cssFont: this.cssFont,
    };
  }
}

/**
 * Structural predicate so cross-realm round-trips (workers, iframes,
 * structuredClone) still classify capability errors correctly — instanceof
 * checks would break the moment the prototype chain is stripped.
 */
export function isMmdsBrowserTextMetricsCapabilityError(
  value: unknown,
): value is MmdsBrowserTextMetricsCapabilityError {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    v.name === "MmdsBrowserTextMetricsCapabilityError" &&
    typeof v.code === "string" &&
    ALL_CODES.has(v.code) &&
    typeof v.message === "string" &&
    typeof v.fallbackEligible === "boolean"
  );
}
