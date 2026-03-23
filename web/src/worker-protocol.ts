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

export type WorkerRequestMessage =
  | WorkerRenderRequestMessage
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

export interface WorkerErrorMessage {
  type: "error";
  seq: number;
  error: string;
}

export type WorkerResponseMessage =
  | WorkerResultMessage
  | WorkerValidationMessage
  | WorkerErrorMessage;
