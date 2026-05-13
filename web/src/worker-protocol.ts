import type { BrowserTextMetricsRequest } from "./browser-text-metrics";

export type WorkerOutputFormat = "text" | "ascii" | "svg" | "mmds" | "mermaid";

export interface WorkerRenderRequestMessage {
  type: "render";
  seq: number;
  input: string;
  format: WorkerOutputFormat;
  configJson: string;
}

export interface WorkerValidateRequestMessage {
  type: "validate";
  seq: number;
  input: string;
}

export interface WorkerDynamicTextMetricsRenderRequestMessage {
  type: "renderWithBrowserTextMetrics";
  seq: number;
  input: string;
  format: "svg";
  configJson: string;
  browserTextMetrics: BrowserTextMetricsRequest;
}

export interface BrowserTextMetricsDecision {
  required: boolean;
  browserTextMetrics?: BrowserTextMetricsRequest;
}

export interface WorkerBrowserTextMetricsRequestMessage {
  type: "resolveBrowserTextMetrics";
  seq: number;
  input: string;
  format: WorkerOutputFormat;
  configJson: string;
}

export type WorkerRequestMessage =
  | WorkerRenderRequestMessage
  | WorkerDynamicTextMetricsRenderRequestMessage
  | WorkerBrowserTextMetricsRequestMessage
  | WorkerValidateRequestMessage;

export interface WorkerResultMessage {
  type: "result";
  seq: number;
  format: WorkerOutputFormat;
  output: string;
}

export interface WorkerValidationMessage {
  type: "validation";
  seq: number;
  resultJson: string;
}

export interface WorkerBrowserTextMetricsDecisionMessage {
  type: "browserTextMetricsDecision";
  seq: number;
  decision: BrowserTextMetricsDecision;
}

export interface WorkerErrorMessage {
  type: "error";
  seq: number;
  error: string;
  code?: "dynamic-metrics-capability";
}

export type WorkerResponseMessage =
  | WorkerResultMessage
  | WorkerValidationMessage
  | WorkerBrowserTextMetricsDecisionMessage
  | WorkerErrorMessage;
