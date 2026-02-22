import { type Diagnostic, linter } from "@codemirror/lint";
import type { Extension } from "@codemirror/state";

interface ValidateDiagnostic {
  severity?: string;
  line?: number;
  column?: number;
  end_line?: number;
  end_column?: number;
  message: string;
}

interface ValidateResult {
  valid: boolean;
  diagnostics?: ValidateDiagnostic[];
}

function computeLineStarts(input: string): number[] {
  const starts = [0];
  for (let index = 0; index < input.length; index += 1) {
    if (input[index] === "\n") {
      starts.push(index + 1);
    }
  }
  return starts;
}

function clamp(value: number, min: number, max: number): number {
  if (value < min) {
    return min;
  }
  if (value > max) {
    return max;
  }
  return value;
}

/**
 * Convert a 1-indexed line number and 1-indexed column to a document offset.
 */
function offsetForPosition(
  lineStarts: readonly number[],
  inputLength: number,
  lineOneBased: number,
  columnOneBased: number,
): number {
  const lineIndex = clamp(
    lineOneBased - 1,
    0,
    Math.max(lineStarts.length - 1, 0),
  );
  const lineStart = lineStarts[lineIndex] ?? inputLength;
  const nextLineStart = lineStarts[lineIndex + 1];
  const lineEnd =
    typeof nextLineStart === "number"
      ? Math.max(lineStart, nextLineStart - 1)
      : inputLength;
  return clamp(lineStart + Math.max(columnOneBased - 1, 0), lineStart, lineEnd);
}

function diagnosticFromValidateEntry(
  input: string,
  lineStarts: readonly number[],
  diag: ValidateDiagnostic,
): Diagnostic {
  const inputLength = input.length;
  let from = 0;
  let to = Math.min(inputLength, 1);

  if (typeof diag.line === "number") {
    from = offsetForPosition(
      lineStarts,
      inputLength,
      diag.line,
      diag.column ?? 1,
    );

    if (
      typeof diag.end_line === "number" &&
      typeof diag.end_column === "number"
    ) {
      to = offsetForPosition(
        lineStarts,
        inputLength,
        diag.end_line,
        diag.end_column,
      );
    } else {
      to = Math.min(inputLength, from + 1);
    }
  }

  if (to <= from && inputLength > 0) {
    to = Math.min(inputLength, from + 1);
  }

  const severity =
    diag.severity === "warning" ? ("warning" as const) : ("error" as const);

  return {
    from,
    to,
    severity,
    source: "mmdflux",
    message: diag.message,
  };
}

export function normalizeValidateResultToDiagnostics(
  input: string,
  validateJson: string,
): Diagnostic[] {
  const result: ValidateResult = JSON.parse(validateJson);
  if (!result.diagnostics?.length) {
    return [];
  }

  const lineStarts = computeLineStarts(input);

  return result.diagnostics.map((diag) =>
    diagnosticFromValidateEntry(input, lineStarts, diag),
  );
}

interface WasmModule {
  default: () => Promise<void>;
  validate: (input: string) => string;
}

let wasmPromise: Promise<WasmModule> | null = null;

async function loadWasm(): Promise<WasmModule> {
  if (!wasmPromise) {
    wasmPromise = import("./wasm-pkg/mmdflux_wasm.js")
      .then(async (module) => {
        const wasm = module as unknown as WasmModule;
        await wasm.default();
        return wasm;
      })
      .catch((error: unknown) => {
        wasmPromise = null;
        throw error;
      });
  }
  return wasmPromise;
}

export async function lintWithWasm(
  input: string,
): Promise<readonly Diagnostic[]> {
  if (input.trim().length === 0) {
    return [];
  }

  const wasm = await loadWasm();
  const resultJson = wasm.validate(input);
  return normalizeValidateResultToDiagnostics(input, resultJson);
}

export const wasmLintExtension: Extension = linter(
  async (view) => lintWithWasm(view.state.doc.toString()),
  { delay: 350 },
);
