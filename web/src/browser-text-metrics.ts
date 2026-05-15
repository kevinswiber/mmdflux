import type { MmdsBrowserTextMetricsErrorCode } from "@mmds/browser-text-metrics";
import {
  isMmdsBrowserTextMetricsCapabilityError,
  MMDS_BROWSER_TEXT_METRICS_PROFILE_ID,
  MmdsBrowserTextMetricsCapabilityError,
} from "@mmds/browser-text-metrics";
import { buildCssFont } from "@mmds/browser-text-metrics/css-font";
import type {
  BrowserTextMetricsEnvironment,
  BrowserTextMetricsRequest,
  BrowserTextMetricsStyleRequest,
  MainThreadBrowserTextMetricsEnvironment,
  PreparedBrowserTextMetrics,
} from "@mmds/browser-text-metrics/prepare";
import {
  prepareMainThreadTextMetrics,
  prepareWorkerTextMetrics,
} from "@mmds/browser-text-metrics/prepare";

export type {
  BrowserTextMetricsEnvironment,
  BrowserTextMetricsRequest,
  BrowserTextMetricsStyleRequest,
  MainThreadBrowserTextMetricsEnvironment,
  PreparedBrowserTextMetrics,
};

export type BrowserTextMetricsCapabilityCode = MmdsBrowserTextMetricsErrorCode;

export const BROWSER_TEXT_METRICS_PROFILE_ID =
  MMDS_BROWSER_TEXT_METRICS_PROFILE_ID;

export {
  buildCssFont,
  isMmdsBrowserTextMetricsCapabilityError as isBrowserTextMetricsCapabilityError,
  MmdsBrowserTextMetricsCapabilityError as BrowserTextMetricsCapabilityError,
};

export function prepareBrowserTextMetrics(
  input: BrowserTextMetricsRequest,
  environment?: BrowserTextMetricsEnvironment,
): Promise<PreparedBrowserTextMetrics> {
  return prepareWorkerTextMetrics({ request: input, environment });
}

export function prepareMainThreadBrowserTextMetrics(
  input: BrowserTextMetricsRequest,
  environment?: MainThreadBrowserTextMetricsEnvironment,
): Promise<PreparedBrowserTextMetrics> {
  return prepareMainThreadTextMetrics({ request: input, environment });
}
