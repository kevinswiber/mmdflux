// Root subpath — pure types + the capability error + the profile constants.
// Importing this entry MUST NOT touch globalThis, evaluate @mmds/wasm, or
// allocate worker/canvas/document — SSR consumers rely on that invariant.

export {
  isMmdsBrowserTextMetricsCapabilityError,
  MmdsBrowserTextMetricsCapabilityError,
  type MmdsBrowserTextMetricsCapabilityErrorArgs,
  type MmdsBrowserTextMetricsErrorCode,
} from "./capability.js";
export type {
  BrowserTextMetricsRequest,
  BrowserTextMetricsStyleRequest,
  PreparedBrowserTextMetrics,
} from "./prepare.js";
export {
  MMDS_BROWSER_TEXT_METRICS_PROFILE_ID,
  MMDS_BROWSER_TEXT_METRICS_PROFILE_VERSION,
} from "./profile.js";
