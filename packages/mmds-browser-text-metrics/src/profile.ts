/**
 * The MMDS browser canvas profile identifier. Both the wasm crate and the
 * package agree on this string so callers can match `metricsProfile.source`
 * across the wire boundary.
 */
export const MMDS_BROWSER_TEXT_METRICS_PROFILE_ID =
  "mmdflux-browser-canvas-v1" as const;

export const MMDS_BROWSER_TEXT_METRICS_PROFILE_VERSION = 1 as const;
