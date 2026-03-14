import {
  EXAMPLE_CATEGORY_LABELS,
  EXAMPLE_CATEGORY_ORDER,
  findExampleById,
  PLAYGROUND_EXAMPLES,
  type PlaygroundExample,
} from "../examples";
import { tokenizeMermaidText } from "../mermaid-language";

interface CreateSnippetGalleryControllerOptions {
  root: HTMLElement;
  onCopySnippet: (example: PlaygroundExample) => void;
  onRunSnippet: (example: PlaygroundExample) => void;
}

export interface SnippetGalleryController {
  render: () => void;
}

const SNIPPET_PREVIEW_LINES = 7;
const SNIPPET_TOKEN_CLASS_BY_TOKEN: Partial<Record<string, string>> = {
  atom: "snippet-token-type",
  keyword: "snippet-token-keyword",
  comment: "snippet-token-comment",
  string: "snippet-token-string",
  number: "snippet-token-number",
  variable: "snippet-token-variable",
  def: "snippet-token-class",
  operator: "snippet-token-transition",
  punctuation: "snippet-token-delimiter",
};

function getSnippetPreview(input: string): string {
  const lines = input.trim().split("\n");
  const previewLines = lines.slice(0, SNIPPET_PREVIEW_LINES);
  if (lines.length > SNIPPET_PREVIEW_LINES) {
    previewLines.push("...");
  }
  return previewLines.join("\n");
}

function renderSnippetPreview(previewBlock: HTMLElement, input: string): void {
  previewBlock.replaceChildren();
  const previewText = getSnippetPreview(input);
  const tokenLines = tokenizeMermaidText(previewText);

  tokenLines.forEach((line, lineIndex) => {
    for (const tokenSpan of line) {
      const className = tokenSpan.token
        ? SNIPPET_TOKEN_CLASS_BY_TOKEN[tokenSpan.token]
        : undefined;
      if (!className) {
        previewBlock.append(document.createTextNode(tokenSpan.text));
        continue;
      }

      const span = document.createElement("span");
      span.className = `snippet-token ${className}`;
      span.textContent = tokenSpan.text;
      previewBlock.append(span);
    }

    if (lineIndex < tokenLines.length - 1) {
      previewBlock.append(document.createTextNode("\n"));
    }
  });
}

export function createSnippetGalleryController(
  options: CreateSnippetGalleryControllerOptions,
): SnippetGalleryController {
  options.root.addEventListener("click", (event) => {
    const target = event.target;
    if (!(target instanceof Element)) {
      return;
    }

    const copyButton = target.closest<HTMLButtonElement>("[data-snippet-copy]");
    if (copyButton) {
      const snippet = findExampleById(copyButton.dataset.snippetCopy ?? "");
      if (snippet) {
        options.onCopySnippet(snippet);
      }
      return;
    }

    const runButton = target.closest<HTMLButtonElement>("[data-snippet-run]");
    if (!runButton) {
      return;
    }

    const snippet = findExampleById(runButton.dataset.snippetRun ?? "");
    if (snippet) {
      options.onRunSnippet(snippet);
    }
  });

  return {
    render: () => {
      options.root.replaceChildren();

      const orderedExamples = [...PLAYGROUND_EXAMPLES].sort((left, right) => {
        if (left.featured !== right.featured) {
          return left.featured ? -1 : 1;
        }
        if (left.category !== right.category) {
          return (
            EXAMPLE_CATEGORY_ORDER.indexOf(left.category) -
            EXAMPLE_CATEGORY_ORDER.indexOf(right.category)
          );
        }
        return left.name.localeCompare(right.name);
      });

      for (const example of orderedExamples.slice(0, 12)) {
        const card = document.createElement("article");
        card.className = "snippet-card";

        const header = document.createElement("div");
        header.className = "snippet-card-header";

        const title = document.createElement("h3");
        title.className = "snippet-title";
        title.textContent = example.name;

        const badge = document.createElement("span");
        badge.className = "snippet-category";
        badge.textContent = EXAMPLE_CATEGORY_LABELS[example.category];

        header.append(title, badge);

        const description = document.createElement("p");
        description.className = "snippet-description";
        description.textContent = example.description;

        const previewBlock = document.createElement("pre");
        previewBlock.className = "snippet-preview";
        renderSnippetPreview(previewBlock, example.input);

        const actionRow = document.createElement("div");
        actionRow.className = "snippet-actions";

        const copyButton = document.createElement("button");
        copyButton.type = "button";
        copyButton.className = "snippet-action-button";
        copyButton.dataset.snippetCopy = example.id;
        copyButton.textContent = "Copy";

        const runButton = document.createElement("button");
        runButton.type = "button";
        runButton.className =
          "snippet-action-button snippet-action-button-primary";
        runButton.dataset.snippetRun = example.id;
        runButton.textContent = "Run in editor";

        actionRow.append(copyButton, runButton);
        card.append(header, description, previewBlock, actionRow);
        options.root.append(card);
      }
    },
  };
}
