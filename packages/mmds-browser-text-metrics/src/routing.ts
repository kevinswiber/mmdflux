export interface MayNeedBrowserTextMetricsInput {
  input: string;
  configJson?: string;
}

/**
 * Opt-in heuristic that returns true when the diagram input or runtime
 * config plausibly declares custom fonts or theme variables. Consumers can
 * short-circuit the wasm resolver round-trip with this; a `false` result
 * does NOT guarantee static rendering will produce identical output if the
 * configJson uses an unsupported theme variable not covered here.
 */
export function mayNeedBrowserTextMetrics(
  options: MayNeedBrowserTextMetricsInput,
): boolean {
  const input = options.input.toLowerCase();
  if (
    input.includes("font-family") ||
    input.includes("font-size") ||
    input.includes("font-style") ||
    input.includes("font-weight")
  ) {
    return true;
  }
  const configJson = options.configJson?.toLowerCase() ?? "";
  return (
    configJson.includes("fontfamily") ||
    configJson.includes("fontsize") ||
    configJson.includes("themevariables")
  );
}
