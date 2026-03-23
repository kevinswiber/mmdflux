import { indentWithTab } from "@codemirror/commands";
import { indentUnit } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { minimalSetup } from "codemirror";
import { mermaidSyntaxHighlighting } from "./mermaid-language";
import {
  createWasmLintExtension,
  type ValidateWithWorker,
} from "./wasm-diagnostics";

export interface EditorController {
  getValue: () => string;
  setValue: (value: string) => void;
  onChange: (listener: (value: string) => void) => () => void;
}

interface CreateEditorControllerOptions {
  root: HTMLElement;
  initialValue: string;
  validateWithWorker?: ValidateWithWorker;
}

function supportsCodeMirrorView(): boolean {
  if (
    typeof document === "undefined" ||
    typeof document.createRange !== "function"
  ) {
    return false;
  }

  try {
    const range = document.createRange();
    return typeof range.getClientRects === "function";
  } catch {
    return false;
  }
}

export function createEditorController(
  options: CreateEditorControllerOptions,
): EditorController {
  const listeners = new Set<(value: string) => void>();
  const emit = (value: string): void => {
    for (const listener of listeners) {
      listener(value);
    }
  };

  const syncTextarea = document.createElement("textarea");
  syncTextarea.className = "editor-input editor-input-sync";
  syncTextarea.value = options.initialValue;
  syncTextarea.setAttribute("aria-label", "Mermaid input");

  const editorRoot = document.createElement("div");
  editorRoot.className = "editor-codemirror";

  let currentValue = options.initialValue;
  let suppressEditorEvents = false;

  const createTextareaFallback = (): EditorController => {
    syncTextarea.className = "editor-input";
    options.root.replaceChildren(syncTextarea);

    syncTextarea.addEventListener("input", () => {
      currentValue = syncTextarea.value;
      emit(currentValue);
    });

    return {
      getValue: () => currentValue,
      setValue: (value: string) => {
        currentValue = value;
        syncTextarea.value = value;
      },
      onChange: (listener) => {
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
        };
      },
    };
  };

  if (!supportsCodeMirrorView()) {
    return createTextareaFallback();
  }

  try {
    const extensions = [
      minimalSetup,
      keymap.of([indentWithTab]),
      EditorView.lineWrapping,
      indentUnit.of("  "),
      ...mermaidSyntaxHighlighting,
    ];
    if (options.validateWithWorker) {
      extensions.push(createWasmLintExtension(options.validateWithWorker));
    }
    extensions.push(
      EditorView.updateListener.of((update) => {
        if (!update.docChanged) {
          return;
        }

        currentValue = update.state.doc.toString();
        syncTextarea.value = currentValue;

        if (!suppressEditorEvents) {
          emit(currentValue);
        }
      }),
    );

    const editorState = EditorState.create({
      doc: currentValue,
      extensions,
    });

    const view = new EditorView({
      state: editorState,
      parent: editorRoot,
    });

    syncTextarea.addEventListener("input", () => {
      const nextValue = syncTextarea.value;
      if (nextValue === currentValue) {
        return;
      }

      currentValue = nextValue;
      suppressEditorEvents = true;
      view.dispatch({
        changes: {
          from: 0,
          to: view.state.doc.length,
          insert: nextValue,
        },
      });
      suppressEditorEvents = false;
      emit(currentValue);
    });

    options.root.replaceChildren(editorRoot, syncTextarea);

    return {
      getValue: () => currentValue,
      setValue: (value: string) => {
        if (value === currentValue) {
          return;
        }

        currentValue = value;
        syncTextarea.value = value;

        suppressEditorEvents = true;
        view.dispatch({
          changes: {
            from: 0,
            to: view.state.doc.length,
            insert: value,
          },
        });
        suppressEditorEvents = false;
      },
      onChange: (listener) => {
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
        };
      },
    };
  } catch {
    return createTextareaFallback();
  }
}
