import { MmdsBrowserTextMetricsCapabilityError } from "./capability.js";

// Substrings emitted by the wasm crate. The reentry guard surfaces the
// literal "wasm has been re-entered" string; the JSON-config rejection
// path surfaces serde errors that either mention the `config_json`
// argument by name or print a "RenderConfig: ..." prefix. Centralizing
// these keeps `classifyWasmError` resilient to wording tweaks: any wasm
// message change should update this table, not the call site.
const WASM_REENTRY_FRAGMENT = "re-entered";
const WASM_CONFIG_FRAGMENT = "config_json";
const WASM_CONFIG_PREFIX = "RenderConfig";

/**
 * Classify a thrown wasm error into a MmdsBrowserTextMetricsCapabilityError so
 * downstream handlers (worker dispatch and the main-thread renderer alike)
 * can react to re-entry, config rejection, and generic render failures
 * consistently. Lives in its own module so the main-thread subpath does
 * not have to import from `./worker`.
 */
export function classifyWasmError(
  error: unknown,
): MmdsBrowserTextMetricsCapabilityError {
  const message = formatError(error);
  if (message.includes(WASM_REENTRY_FRAGMENT)) {
    return new MmdsBrowserTextMetricsCapabilityError({
      code: "wasm-reentered",
      message,
      fallbackEligible: false,
      cause: error,
    });
  }
  if (
    message.includes(WASM_CONFIG_FRAGMENT) ||
    message.startsWith(WASM_CONFIG_PREFIX)
  ) {
    return new MmdsBrowserTextMetricsCapabilityError({
      code: "wasm-config-rejected",
      message,
      fallbackEligible: false,
      cause: error,
    });
  }
  return new MmdsBrowserTextMetricsCapabilityError({
    code: "wasm-render-rejected",
    message,
    fallbackEligible: false,
    cause: error,
  });
}

/**
 * Run a synchronous wasm call and classify any throw as a typed capability
 * error. Lives alongside `classifyWasmError` because every caller pairs
 * the two.
 */
export function runWasm<T>(call: () => T): T {
  try {
    return call();
  } catch (error) {
    throw classifyWasmError(error);
  }
}

function formatError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}
