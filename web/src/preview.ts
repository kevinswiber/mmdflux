import { escapeAnsiForDisplay, parseAnsiSegments, stripAnsi } from "./ansi";
import type { WorkerOutputFormat } from "./worker-protocol";

interface PreviewElements {
  output: HTMLElement;
  error: HTMLElement;
}

interface PreviewResult {
  format: WorkerOutputFormat;
  output: string;
}

export type TextPreviewMode = "plain" | "styled" | "ansi";

export type PreviewCopyKind = "plain" | "ansi";

export interface PreviewController {
  showResult: (result: PreviewResult) => void;
  showError: (message: string) => void;
  setTextMode: (mode: TextPreviewMode) => void;
  getCopyText: (kind: PreviewCopyKind) => string | null;
}

export function createPreviewController(
  elements: PreviewElements,
): PreviewController {
  let lastResult: PreviewResult | null = null;
  let textMode: TextPreviewMode = "plain";

  const hideError = (): void => {
    elements.error.hidden = true;
    elements.error.textContent = "";
  };

  const renderCurrentResult = (): void => {
    if (!lastResult) {
      return;
    }

    hideError();
    elements.output.classList.toggle("is-svg", lastResult.format === "svg");
    elements.output.classList.toggle(
      "is-terminal",
      lastResult.format === "text" && textMode === "styled",
    );

    if (lastResult.format === "svg") {
      elements.output.innerHTML = lastResult.output;
      return;
    }

    if (lastResult.format === "text") {
      renderTextOutput(elements.output, lastResult.output, textMode);
      return;
    }

    elements.output.textContent = lastResult.output;
  };

  return {
    showResult: (result) => {
      lastResult = result;
      renderCurrentResult();
    },
    showError: (message) => {
      elements.error.hidden = false;
      elements.error.textContent = `Render error: ${message}`;
    },
    setTextMode: (mode) => {
      textMode = mode;
      renderCurrentResult();
    },
    getCopyText: (kind) => {
      if (!lastResult || lastResult.format !== "text") {
        return null;
      }

      return kind === "plain"
        ? stripAnsi(lastResult.output)
        : lastResult.output;
    },
  };
}

function renderTextOutput(
  outputRoot: HTMLElement,
  rawOutput: string,
  mode: TextPreviewMode,
): void {
  if (mode === "plain") {
    outputRoot.textContent = stripAnsi(rawOutput);
    return;
  }

  if (mode === "ansi") {
    outputRoot.textContent = escapeAnsiForDisplay(rawOutput);
    return;
  }

  const documentRef = outputRoot.ownerDocument ?? document;
  const pre = documentRef.createElement("pre");
  pre.className = "ansi-preview";

  for (const segment of parseAnsiSegments(rawOutput)) {
    if (!segment.style.foreground && !segment.style.background) {
      pre.append(documentRef.createTextNode(segment.text));
      continue;
    }

    const span = documentRef.createElement("span");
    span.textContent = segment.text;
    if (segment.style.foreground) {
      span.style.color = segment.style.foreground;
    }
    if (segment.style.background) {
      span.style.backgroundColor = segment.style.background;
    }
    pre.append(span);
  }

  outputRoot.replaceChildren(pre);
}
