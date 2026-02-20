import type { WorkerOutputFormat } from "./worker-protocol";

interface PreviewElements {
  output: HTMLElement;
  error: HTMLElement;
}

interface PreviewResult {
  format: WorkerOutputFormat;
  output: string;
}

export interface PreviewController {
  showResult: (result: PreviewResult) => void;
  showError: (message: string) => void;
}

export function createPreviewController(
  elements: PreviewElements,
): PreviewController {
  const hideError = (): void => {
    elements.error.hidden = true;
    elements.error.textContent = "";
  };

  return {
    showResult: (result) => {
      hideError();
      elements.output.classList.toggle("is-svg", result.format === "svg");
      if (result.format === "svg") {
        elements.output.innerHTML = result.output;
        return;
      }

      elements.output.textContent = result.output;
    },
    showError: (message) => {
      elements.error.hidden = false;
      elements.error.textContent = `Render error: ${message}`;
    },
  };
}
