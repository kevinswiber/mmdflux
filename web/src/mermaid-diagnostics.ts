import { type Diagnostic, linter } from "@codemirror/lint";
import type { Extension } from "@codemirror/state";
import type { DetailedError, Mermaid, MermaidConfig } from "mermaid";

interface MermaidParserLocation {
  first_line?: number;
  last_line?: number;
  first_column?: number;
  last_column?: number;
}

interface MermaidParserHash {
  loc?: MermaidParserLocation;
  line?: number;
  text?: string;
}

const MERMAID_LINT_INIT_CONFIG: MermaidConfig = {
  startOnLoad: false,
  suppressErrorRendering: true,
  logLevel: "fatal",
};

let mermaidPromise: Promise<Mermaid> | null = null;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isDetailedErrorLike(error: unknown): error is DetailedError {
  return (
    isRecord(error) &&
    typeof error.str === "string" &&
    "hash" in error &&
    isRecord(error.hash)
  );
}

function normalizeMessage(rawMessage: string): string {
  const lines = rawMessage
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !/^[-^~`]+$/.test(line));

  if (lines.length === 0) {
    return "Invalid Mermaid syntax.";
  }

  const firstLine = lines[0] ?? "Invalid Mermaid syntax.";
  const expectingLine = lines.find((line) => line.startsWith("Expecting "));
  if (expectingLine && expectingLine !== firstLine) {
    return `${firstLine} ${expectingLine}`;
  }

  return firstLine;
}

function extractRawMessage(error: unknown): string {
  if (isDetailedErrorLike(error)) {
    return error.str;
  }

  if (isRecord(error) && typeof error.message === "string") {
    return error.message;
  }

  if (typeof error === "string") {
    return error;
  }

  return "Invalid Mermaid syntax.";
}

function extractHash(error: unknown): MermaidParserHash | undefined {
  if (isDetailedErrorLike(error)) {
    return error.hash as MermaidParserHash;
  }

  if (isRecord(error) && isRecord(error.hash)) {
    return error.hash as MermaidParserHash;
  }

  return undefined;
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

function lineStartOffset(
  lineStarts: readonly number[],
  inputLength: number,
  lineOneBased: number,
): number {
  const lineIndex = clamp(
    lineOneBased - 1,
    0,
    Math.max(lineStarts.length - 1, 0),
  );
  return lineStarts[lineIndex] ?? inputLength;
}

function lineEndOffset(
  lineStarts: readonly number[],
  inputLength: number,
  lineOneBased: number,
): number {
  const lineIndex = clamp(
    lineOneBased - 1,
    0,
    Math.max(lineStarts.length - 1, 0),
  );
  const nextLineStart = lineStarts[lineIndex + 1];
  if (typeof nextLineStart !== "number") {
    return inputLength;
  }

  return Math.max(lineStarts[lineIndex] ?? 0, nextLineStart - 1);
}

function offsetForPosition(
  lineStarts: readonly number[],
  inputLength: number,
  lineOneBased: number,
  columnZeroBased: number,
): number {
  const start = lineStartOffset(lineStarts, inputLength, lineOneBased);
  const end = lineEndOffset(lineStarts, inputLength, lineOneBased);
  return clamp(
    start + Math.max(columnZeroBased, 0),
    start,
    Math.max(start, end),
  );
}

function parseLineFromMessage(message: string): number | undefined {
  const lineMatch = message.match(/line\s+(\d+)/i);
  if (!lineMatch) {
    return undefined;
  }

  const parsedLine = Number(lineMatch[1]);
  if (!Number.isFinite(parsedLine) || parsedLine <= 0) {
    return undefined;
  }
  return parsedLine;
}

function diagnosticRange(
  input: string,
  error: unknown,
): { from: number; to: number } {
  const hash = extractHash(error);
  const rawMessage = extractRawMessage(error);
  const lineStarts = computeLineStarts(input);
  const inputLength = input.length;

  const highlightLength =
    hash && typeof hash.text === "string" && hash.text.length > 0
      ? hash.text.length
      : 1;

  let startLine: number | undefined;
  let startColumn: number | undefined;
  let endLine: number | undefined;
  let endColumn: number | undefined;

  if (hash?.loc) {
    const { loc } = hash;
    if (typeof loc.first_line === "number") {
      startLine = loc.first_line;
    }
    if (typeof loc.first_column === "number") {
      startColumn = loc.first_column;
    }
    if (typeof loc.last_line === "number") {
      endLine = loc.last_line;
    }
    if (typeof loc.last_column === "number") {
      endColumn = loc.last_column;
    }
  }

  if (typeof startLine !== "number" && typeof hash?.line === "number") {
    startLine = hash.line + 1;
  }

  if (typeof startLine !== "number") {
    startLine = parseLineFromMessage(rawMessage);
  }

  if (typeof startLine !== "number") {
    if (inputLength === 0) {
      return { from: 0, to: 0 };
    }
    return { from: 0, to: Math.min(inputLength, highlightLength) };
  }

  const normalizedStartColumn =
    typeof startColumn === "number" ? startColumn : 0;
  const normalizedEndLine = typeof endLine === "number" ? endLine : startLine;
  const normalizedEndColumn =
    typeof endColumn === "number"
      ? endColumn
      : normalizedStartColumn + highlightLength;

  const from = offsetForPosition(
    lineStarts,
    inputLength,
    startLine,
    normalizedStartColumn,
  );
  let to = offsetForPosition(
    lineStarts,
    inputLength,
    normalizedEndLine,
    normalizedEndColumn,
  );

  if (to <= from && inputLength > 0) {
    to = Math.min(inputLength, from + Math.max(highlightLength, 1));
  }

  return { from, to };
}

export function normalizeMermaidErrorToDiagnostic(
  input: string,
  error: unknown,
): Diagnostic {
  const { from, to } = diagnosticRange(input, error);

  return {
    from,
    to,
    severity: "error",
    source: "mermaid",
    message: normalizeMessage(extractRawMessage(error)),
  };
}

async function loadMermaid(): Promise<Mermaid> {
  if (!mermaidPromise) {
    mermaidPromise = import("mermaid")
      .then((module) => {
        const mermaid = module.default;
        mermaid.initialize(MERMAID_LINT_INIT_CONFIG);
        return mermaid;
      })
      .catch((error: unknown) => {
        mermaidPromise = null;
        throw error;
      });
  }

  return mermaidPromise;
}

export async function lintMermaidSource(
  input: string,
): Promise<readonly Diagnostic[]> {
  if (input.trim().length === 0) {
    return [];
  }

  try {
    const mermaid = await loadMermaid();
    await mermaid.parse(input, { suppressErrors: false });
    return [];
  } catch (error) {
    return [normalizeMermaidErrorToDiagnostic(input, error)];
  }
}

export const mermaidLintExtension: Extension = linter(
  async (view) => lintMermaidSource(view.state.doc.toString()),
  {
    delay: 350,
  },
);
