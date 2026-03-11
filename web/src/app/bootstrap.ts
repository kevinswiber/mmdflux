import { createEditorController } from "../editor";
import {
  DEFAULT_EXAMPLE_ID,
  EXAMPLE_CATEGORY_LABELS,
  EXAMPLE_CATEGORY_ORDER,
  findExampleById,
  PLAYGROUND_EXAMPLES,
} from "../examples";
import { createSnippetGalleryController } from "../features/snippet-gallery";
import {
  helpText,
  isSupported,
  type RenderControlId,
} from "../format-capabilities";
import {
  createLiveUpdateController,
  type LiveUpdateDebounceSetting,
} from "../live-update";
import {
  createPreviewController,
  type PreviewCopyKind,
  type TextPreviewMode,
} from "../preview";
import { createPreviewControls } from "../preview-controls";
import {
  persistPlaygroundState,
  readPersistedPlaygroundState,
  resolveStateStorage,
  type StateStorage,
} from "../services/playground-state";
import {
  createRenderWorkerClient,
  type RenderWorkerClient,
} from "../services/render-client";
import {
  copyTextToClipboard,
  createShareStateService,
} from "../services/share-state";
import {
  DEFAULT_SHARE_RENDER_SETTINGS,
  type ShareEdgePreset,
  type ShareLayoutEngine,
  type SharePathSimplification,
} from "../share";
import { createThemeController, type ThemePreference } from "../theme";
import {
  createPlaygroundStateStore,
  DRAFT_EXAMPLE_ID,
  type ExampleSelectionId,
  isPlaygroundFormat,
  type PlaygroundFormat,
} from "./state";

interface RenderControlBinding {
  control: RenderControlId;
  select: HTMLSelectElement;
  help: HTMLElement;
  container: HTMLElement;
}

export interface RenderAppOptions {
  renderClientFactory?: () => RenderWorkerClient | null;
  debounceMs?: LiveUpdateDebounceSetting;
  stateStorage?: StateStorage;
}

type SearchLocation = URL | Pick<Location, "search">;

function defaultAdaptiveDebounce(requestInput: string): number {
  const length = requestInput.length;
  if (length <= 2_500) {
    return 0;
  }
  if (length <= 8_000) {
    return 40;
  }
  if (length <= 16_000) {
    return 80;
  }
  return 120;
}

function isLayoutEngine(value: string): value is ShareLayoutEngine {
  return (
    value === "auto" || value === "flux-layered" || value === "mermaid-layered"
  );
}

function isEdgePreset(value: string): value is ShareEdgePreset {
  return (
    value === "auto" ||
    value === "straight" ||
    value === "step" ||
    value === "smooth-step" ||
    value === "curved-step" ||
    value === "basis"
  );
}

function isPathSimplification(value: string): value is SharePathSimplification {
  return (
    value === "none" ||
    value === "lossless" ||
    value === "lossy" ||
    value === "minimal"
  );
}

function isTextPreviewMode(value: string): value is TextPreviewMode {
  return value === "plain" || value === "styled" || value === "ansi";
}

function nextThemePreference(current: ThemePreference): ThemePreference {
  if (current === "system") {
    return "light";
  }
  if (current === "light") {
    return "dark";
  }
  return "system";
}

const THEME_LABELS: Record<ThemePreference, string> = {
  system: "System",
  light: "Light",
  dark: "Dark",
};

const THEME_ICONS: Record<ThemePreference, string> = {
  light:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4"></circle><path d="M12 2.75V5.25"></path><path d="M12 18.75V21.25"></path><path d="M4.75 12H2.75"></path><path d="M21.25 12H19.25"></path><path d="M6.86 6.86L5.1 5.1"></path><path d="M18.9 18.9L17.14 17.14"></path><path d="M17.14 6.86L18.9 5.1"></path><path d="M5.1 18.9L6.86 17.14"></path></svg>',
  dark: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14.5 3.3a8.7 8.7 0 1 0 6.2 14.8A9.2 9.2 0 0 1 14.5 3.3Z"></path></svg>',
  system:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="12" rx="2"></rect><path d="M9 20h6"></path><path d="M12 16v4"></path></svg>',
};

function viewportHeight(): number {
  return window.innerHeight || document.documentElement.clientHeight || 0;
}

function visibleHeightInViewport(rect: DOMRect): number {
  const viewHeight = viewportHeight();
  if (viewHeight <= 0) {
    return 0;
  }

  const top = Math.max(rect.top, 0);
  const bottom = Math.min(rect.bottom, viewHeight);
  return Math.max(0, bottom - top);
}

function workspaceMostlyOffscreen(workspace: HTMLElement): boolean {
  const rect = workspace.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) {
    return false;
  }

  const viewHeight = viewportHeight();
  if (viewHeight <= 0) {
    return false;
  }

  const visibleHeight = visibleHeightInViewport(rect);
  const effectiveHeight = Math.min(rect.height, viewHeight);
  if (effectiveHeight <= 0) {
    return false;
  }

  const visibleRatio = visibleHeight / effectiveHeight;
  return visibleRatio < 0.6;
}

function scrollWorkspaceIntoView(workspace: HTMLElement): void {
  if (!workspaceMostlyOffscreen(workspace)) {
    return;
  }

  const prefersReducedMotion =
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  workspace.scrollIntoView({
    behavior: prefersReducedMotion ? "auto" : "smooth",
    block: "start",
    inline: "nearest",
  });
}

function updateThemeToggleButton(
  button: HTMLButtonElement,
  preference: ThemePreference,
): void {
  const nextPreference = nextThemePreference(preference);
  const currentLabel = THEME_LABELS[preference];
  const nextLabel = THEME_LABELS[nextPreference];
  button.dataset.themeMode = preference;
  button.setAttribute(
    "aria-label",
    `Theme: ${currentLabel}. Switch to ${nextLabel}.`,
  );
  button.title = `Theme: ${currentLabel}.`;
  button.innerHTML = THEME_ICONS[preference];
}

function populateExampleSelect(select: HTMLSelectElement): void {
  select.replaceChildren();

  const customOption = document.createElement("option");
  customOption.value = DRAFT_EXAMPLE_ID;
  customOption.textContent = "Draft";
  customOption.dataset.custom = "true";
  select.append(customOption);

  for (const category of EXAMPLE_CATEGORY_ORDER) {
    const group = document.createElement("optgroup");
    group.label = EXAMPLE_CATEGORY_LABELS[category];

    const examples = PLAYGROUND_EXAMPLES.filter(
      (example) => example.category === category,
    );
    for (const example of examples) {
      const option = document.createElement("option");
      option.value = example.id;
      option.textContent = `${example.name} · ${example.description}`;
      group.append(option);
    }

    select.append(group);
  }
}

export function renderApp(
  root: HTMLElement,
  options: RenderAppOptions = {},
): void {
  const shareStateService = createShareStateService();
  const stateStorage = resolveStateStorage(options.stateStorage);
  const restoredShareState = shareStateService.readState();
  const restoredLocalState = readPersistedPlaygroundState(stateStorage, {
    findExampleIdByInput: (input) =>
      PLAYGROUND_EXAMPLES.find((example) => example.input === input)?.id ??
      null,
    isKnownExampleId: (id) => Boolean(findExampleById(id)),
  });
  const defaultExample =
    findExampleById(DEFAULT_EXAMPLE_ID) ?? PLAYGROUND_EXAMPLES[0];
  const sharedExampleMatch = restoredShareState
    ? PLAYGROUND_EXAMPLES.find(
        (example) => example.input === restoredShareState.input,
      )
    : null;
  const initialSelectedExampleId: ExampleSelectionId = restoredShareState
    ? (sharedExampleMatch?.id ?? DRAFT_EXAMPLE_ID)
    : (restoredLocalState?.selectedExampleId ?? DRAFT_EXAMPLE_ID);
  const initialDraftInput =
    restoredLocalState?.customInput ??
    restoredShareState?.input ??
    defaultExample?.input ??
    "";
  const initialInput =
    restoredShareState?.input ?? restoredLocalState?.input ?? initialDraftInput;
  const initialFormat =
    restoredShareState?.format ?? restoredLocalState?.format ?? "svg";
  const initialRenderSettings =
    restoredShareState?.renderSettings ??
    restoredLocalState?.renderSettings ??
    DEFAULT_SHARE_RENDER_SETTINGS;
  const initialTextPreviewMode =
    restoredShareState?.textPreviewMode ??
    restoredLocalState?.textPreviewMode ??
    "plain";
  const stateStore = createPlaygroundStateStore({
    initialState: {
      input: initialInput,
      format: initialFormat,
      renderSettings: initialRenderSettings,
      textPreviewMode: initialTextPreviewMode,
      selectedExampleId: initialSelectedExampleId,
      customInput: initialDraftInput,
      advancedOpen: false,
    },
  });

  root.innerHTML = `
    <main class="playground playground-app">
      <header class="toolbar">
        <div class="toolbar-title-group">
          <h1>mmdflux playground <a href="https://github.com/kevinswiber/mmdflux" target="_blank" rel="noopener noreferrer" class="repo-link">kevinswiber/mmdflux</a></h1>
          <div class="toolbar-title-actions">
            <button type="button" class="toolbar-button theme-cycler theme-cycler-subtle" data-theme-toggle aria-live="polite"></button>
          </div>
        </div>
        <div class="toolbar-actions toolbar-actions-primary">
          <div class="toolbar-actions-left">
            <label class="example-picker">
              <span>Example</span>
              <select data-example-select></select>
            </label>
            <div class="format-tabs" role="tablist" aria-label="Output format">
              <button type="button" role="tab" data-format="text" aria-selected="true" class="is-active">Text</button>
              <button type="button" role="tab" data-format="svg" aria-selected="false">SVG</button>
              <button type="button" role="tab" data-format="mmds" aria-selected="false">MMDS</button>
            </div>
            <button type="button" class="toolbar-button toolbar-button-toggle" data-advanced-toggle aria-expanded="false" aria-controls="advanced-controls-panel">Advanced controls</button>
            <div class="export-control">
              <button type="button" class="toolbar-button" data-export-toggle hidden>Export</button>
              <div class="export-menu" data-export-menu hidden>
                <button type="button" data-export-svg>Download SVG</button>
                <button type="button" data-export-png>Download PNG</button>
              </div>
            </div>
          </div>
          <div class="toolbar-actions-right">
            <button type="button" class="toolbar-button" data-share>Copy Share URL</button>
          </div>
        </div>
      </header>

      <section id="advanced-controls-panel" class="advanced-panel" data-advanced-panel hidden>
        <h2>Render Settings</h2>
        <div class="render-settings-grid">
          <div class="render-setting" data-setting="layoutEngine">
            <label for="layout-engine-select">Layout Engine</label>
            <select id="layout-engine-select" data-layout-engine>
              <option value="auto">Auto</option>
              <option value="flux-layered">flux-layered</option>
              <option value="mermaid-layered">mermaid-layered</option>
            </select>
            <p class="render-help" data-help-layout-engine></p>
          </div>
          <div class="render-setting" data-setting="edgePreset">
            <label for="edge-preset-select">Edge Preset</label>
            <select id="edge-preset-select" data-edge-preset>
              <option value="auto">Auto</option>
              <option value="straight">straight</option>
              <option value="step">step</option>
              <option value="smooth-step">smooth-step</option>
              <option value="curved-step">curved-step</option>
              <option value="basis">basis</option>
            </select>
            <p class="render-help" data-help-edge-preset></p>
          </div>
          <div class="render-setting" data-setting="pathSimplification">
            <label for="path-simplification-select">Path Simplification</label>
            <select id="path-simplification-select" data-path-simplification>
              <option value="none">none</option>
              <option value="lossless">lossless</option>
              <option value="lossy">lossy</option>
              <option value="minimal">minimal</option>
            </select>
            <p class="render-help" data-help-path-simplification></p>
          </div>
        </div>
      </section>

      <section class="workspace">
        <div class="panel">
          <h2>Input</h2>
          <div data-editor-root></div>
          <p class="editor-status" data-editor-status hidden></p>
          <p class="preview-error" data-preview-error hidden></p>
        </div>
        <div class="panel">
          <div class="panel-header">
            <h2>Preview</h2>
            <div class="text-preview-toolbar" data-text-preview-toolbar hidden>
              <div class="preview-mode-tabs" role="tablist" aria-label="Text preview mode">
                <button type="button" role="tab" class="is-active" data-text-preview-mode="plain" aria-selected="true">Plain</button>
                <button type="button" role="tab" data-text-preview-mode="styled" aria-selected="false">Styled</button>
                <button type="button" role="tab" data-text-preview-mode="ansi" aria-selected="false">ANSI</button>
              </div>
              <div class="preview-text-actions">
                <button type="button" class="preview-text-action" data-copy-plain>Copy plain</button>
                <button type="button" class="preview-text-action" data-copy-ansi title="Copy raw ANSI escape sequences">Copy ANSI</button>
              </div>
            </div>
          </div>
          <p class="share-status" data-share-status hidden></p>
          <div class="preview-stage" data-preview-stage>
            <div class="preview-controls-overlay" data-preview-controls-overlay hidden>
              <button
                type="button"
                class="preview-controls-toggle"
                data-preview-controls-toggle
                aria-expanded="false"
                aria-label="Show zoom controls"
                title="Show zoom controls"
              >
                <svg class="preview-controls-toggle-icon" viewBox="0 0 24 24" aria-hidden="true">
                  <circle cx="11" cy="11" r="6"></circle>
                  <path d="M20 20L16.2 16.2"></path>
                </svg>
              </button>
              <div class="preview-toolbar" data-preview-controls hidden>
                <button type="button" class="preview-toolbar-button" data-zoom-out>-</button>
                <span class="preview-zoom-label" data-zoom-label>100%</span>
                <button type="button" class="preview-toolbar-button" data-zoom-in>+</button>
                <button type="button" class="preview-toolbar-button" data-zoom-fit>Fit</button>
                <button type="button" class="preview-toolbar-button" data-zoom-reset>100%</button>
              </div>
            </div>
            <div class="preview-output" data-preview-output></div>
          </div>
        </div>
      </section>

      <section class="snippet-gallery">
        <div class="snippet-gallery-header">
          <h2>Syntax snippets</h2>
          <p>Browse curated examples, copy code, or run directly in the editor.</p>
        </div>
        <div class="snippet-grid" data-snippet-grid></div>
      </section>
    </main>
  `;

  const editorRoot = root.querySelector<HTMLElement>("[data-editor-root]");
  const previewStage = root.querySelector<HTMLElement>("[data-preview-stage]");
  const previewOutput = root.querySelector<HTMLElement>(
    "[data-preview-output]",
  );
  const editorStatus = root.querySelector<HTMLElement>("[data-editor-status]");
  const previewError = root.querySelector<HTMLElement>("[data-preview-error]");
  const shareStatus = root.querySelector<HTMLElement>("[data-share-status]");
  const shareButton = root.querySelector<HTMLButtonElement>("[data-share]");
  const themeToggleButton = root.querySelector<HTMLButtonElement>(
    "[data-theme-toggle]",
  );
  const exampleSelect = root.querySelector<HTMLSelectElement>(
    "[data-example-select]",
  );
  const formatButtons = root.querySelectorAll<HTMLButtonElement>(
    ".format-tabs button[data-format]",
  );
  const advancedToggleButton = root.querySelector<HTMLButtonElement>(
    "[data-advanced-toggle]",
  );
  const advancedPanel = root.querySelector<HTMLElement>(
    "[data-advanced-panel]",
  );
  const workspace = root.querySelector<HTMLElement>(".workspace");
  const snippetGrid = root.querySelector<HTMLElement>("[data-snippet-grid]");

  const layoutEngineSelect = root.querySelector<HTMLSelectElement>(
    "[data-layout-engine]",
  );
  const edgePresetSelect =
    root.querySelector<HTMLSelectElement>("[data-edge-preset]");
  const pathSimplificationSelect = root.querySelector<HTMLSelectElement>(
    "[data-path-simplification]",
  );

  const layoutHelp = root.querySelector<HTMLElement>(
    "[data-help-layout-engine]",
  );
  const edgeHelp = root.querySelector<HTMLElement>("[data-help-edge-preset]");
  const pathHelp = root.querySelector<HTMLElement>(
    "[data-help-path-simplification]",
  );

  const layoutSetting = root.querySelector<HTMLElement>(
    '[data-setting="layoutEngine"]',
  );
  const edgeSetting = root.querySelector<HTMLElement>(
    '[data-setting="edgePreset"]',
  );
  const pathSetting = root.querySelector<HTMLElement>(
    '[data-setting="pathSimplification"]',
  );

  const previewControlsOverlayRoot = root.querySelector<HTMLElement>(
    "[data-preview-controls-overlay]",
  );
  const previewControlsToggleButton = root.querySelector<HTMLButtonElement>(
    "[data-preview-controls-toggle]",
  );
  const previewControlsRoot = root.querySelector<HTMLElement>(
    "[data-preview-controls]",
  );
  const textPreviewToolbar = root.querySelector<HTMLElement>(
    "[data-text-preview-toolbar]",
  );
  const textPreviewModeButtons = root.querySelectorAll<HTMLButtonElement>(
    "[data-text-preview-mode]",
  );
  const copyPlainButton =
    root.querySelector<HTMLButtonElement>("[data-copy-plain]");
  const copyAnsiButton =
    root.querySelector<HTMLButtonElement>("[data-copy-ansi]");
  const zoomOutButton =
    root.querySelector<HTMLButtonElement>("[data-zoom-out]");
  const zoomInButton = root.querySelector<HTMLButtonElement>("[data-zoom-in]");
  const zoomFitButton =
    root.querySelector<HTMLButtonElement>("[data-zoom-fit]");
  const zoomResetButton =
    root.querySelector<HTMLButtonElement>("[data-zoom-reset]");
  const zoomLabel = root.querySelector<HTMLElement>("[data-zoom-label]");

  const exportToggleButton = root.querySelector<HTMLButtonElement>(
    "[data-export-toggle]",
  );
  const exportMenu = root.querySelector<HTMLElement>("[data-export-menu]");
  const exportSvgButton =
    root.querySelector<HTMLButtonElement>("[data-export-svg]");
  const exportPngButton =
    root.querySelector<HTMLButtonElement>("[data-export-png]");

  if (
    !editorRoot ||
    !previewStage ||
    !previewOutput ||
    !editorStatus ||
    !previewError ||
    !shareStatus ||
    !shareButton ||
    !themeToggleButton ||
    !exampleSelect ||
    !advancedToggleButton ||
    !advancedPanel ||
    !workspace ||
    !snippetGrid ||
    !layoutEngineSelect ||
    !edgePresetSelect ||
    !pathSimplificationSelect ||
    !layoutHelp ||
    !edgeHelp ||
    !pathHelp ||
    !layoutSetting ||
    !edgeSetting ||
    !pathSetting ||
    !previewControlsOverlayRoot ||
    !previewControlsToggleButton ||
    !previewControlsRoot ||
    !textPreviewToolbar ||
    textPreviewModeButtons.length === 0 ||
    !copyPlainButton ||
    !copyAnsiButton ||
    !zoomOutButton ||
    !zoomInButton ||
    !zoomFitButton ||
    !zoomResetButton ||
    !zoomLabel ||
    !exportToggleButton ||
    !exportMenu ||
    !exportSvgButton ||
    !exportPngButton
  ) {
    return;
  }

  const preview = createPreviewController({
    output: previewOutput,
    error: previewError,
  });
  const editor = createEditorController({
    root: editorRoot,
    initialValue: stateStore.getState().input,
  });
  const previewControls = createPreviewControls({
    viewportRoot: previewOutput,
    controlsOverlayRoot: previewControlsOverlayRoot,
    controlsToggleButton: previewControlsToggleButton,
    controlsRoot: previewControlsRoot,
    zoomOutButton,
    zoomInButton,
    zoomFitButton,
    zoomResetButton,
    zoomLabel,
    exportToggleButton,
    exportMenu,
    exportSvgButton,
    exportPngButton,
  });
  previewControls.attachTo(previewOutput);

  populateExampleSelect(exampleSelect);
  const selectedInitialExample =
    stateStore.getState().selectedExampleId === DRAFT_EXAMPLE_ID
      ? null
      : findExampleById(stateStore.getState().selectedExampleId);
  exampleSelect.value = selectedInitialExample?.id ?? DRAFT_EXAMPLE_ID;

  const matchMedia =
    typeof window.matchMedia === "function"
      ? window.matchMedia.bind(window)
      : undefined;
  const themeStorage = (() => {
    try {
      return window.localStorage;
    } catch {
      return undefined;
    }
  })();
  const themeController = createThemeController({
    root: document.documentElement,
    storage: themeStorage,
    matchMedia,
  });
  themeController.apply();
  updateThemeToggleButton(themeToggleButton, themeController.getPreference());

  const workerClient = options.renderClientFactory
    ? options.renderClientFactory()
    : typeof Worker === "undefined"
      ? null
      : createRenderWorkerClient();

  const renderControlBindings: RenderControlBinding[] = [
    {
      control: "layoutEngine",
      select: layoutEngineSelect,
      help: layoutHelp,
      container: layoutSetting,
    },
    {
      control: "edgePreset",
      select: edgePresetSelect,
      help: edgeHelp,
      container: edgeSetting,
    },
    {
      control: "pathSimplification",
      select: pathSimplificationSelect,
      help: pathHelp,
      container: pathSetting,
    },
  ];

  const updateShareStatus = (message: string): void => {
    shareStatus.hidden = false;
    shareStatus.textContent = message;
  };

  const updateEditorStatus = (message: string): void => {
    editorStatus.hidden = false;
    editorStatus.textContent = message;
  };

  const clearEditorStatus = (): void => {
    editorStatus.hidden = true;
    editorStatus.textContent = "";
  };

  const syncSelectionOnEditorInput = (input: string): void => {
    const currentState = stateStore.getState();
    if (currentState.selectedExampleId === DRAFT_EXAMPLE_ID) {
      stateStore.setCustomInput(input);
      return;
    }

    const selectedExample = findExampleById(currentState.selectedExampleId);
    if (!selectedExample || input !== selectedExample.input) {
      stateStore.selectExample(DRAFT_EXAMPLE_ID);
      exampleSelect.value = DRAFT_EXAMPLE_ID;
      stateStore.setCustomInput(input);
    }
  };

  previewControls.setStatusReporter((message) => {
    updateShareStatus(message);
  });

  const applyRenderSettingsToControls = (): void => {
    const { renderSettings } = stateStore.getState();
    layoutEngineSelect.value = renderSettings.layoutEngine;
    edgePresetSelect.value = renderSettings.edgePreset;
    pathSimplificationSelect.value = renderSettings.pathSimplification;
  };

  const applyRenderControlState = (): void => {
    const { format } = stateStore.getState();
    for (const binding of renderControlBindings) {
      const supported = isSupported(format, binding.control);
      binding.select.disabled = !supported;
      binding.help.textContent = helpText(format, binding.control);
      binding.container.classList.toggle("is-disabled", !supported);
    }
  };

  const applyTextPreviewModeState = (): void => {
    const { format, textPreviewMode } = stateStore.getState();
    const visible = format === "text";
    textPreviewToolbar.hidden = !visible;
    copyPlainButton.disabled = !visible;
    copyAnsiButton.disabled = !visible;

    for (const button of textPreviewModeButtons) {
      const active = button.dataset.textPreviewMode === textPreviewMode;
      button.classList.toggle("is-active", active);
      button.setAttribute("aria-selected", String(active));
    }
  };

  const currentConfigJson = (): string => {
    const { format, renderSettings } = stateStore.getState();
    const config: Record<string, string> = {};
    if (renderSettings.layoutEngine !== "auto") {
      config.layoutEngine = renderSettings.layoutEngine;
    }

    if (format === "text") {
      config.color = "always";
    }

    if (format === "svg") {
      if (renderSettings.edgePreset !== "auto") {
        config.edgePreset = renderSettings.edgePreset;
      }
    }

    if (format === "mmds") {
      config.geometryLevel = "routed";
    }

    if (format === "svg" || format === "mmds") {
      config.pathSimplification = renderSettings.pathSimplification;
    }

    return JSON.stringify(config);
  };

  const setAdvancedPanelOpen = (open: boolean): void => {
    stateStore.setAdvancedOpen(open);
    advancedPanel.hidden = !open;
    advancedToggleButton.setAttribute("aria-expanded", String(open));
    advancedToggleButton.classList.toggle("is-active", open);
  };

  const setFormat = (format: PlaygroundFormat): void => {
    stateStore.selectFormat(format);
    for (const button of formatButtons) {
      const active = button.dataset.format === format;
      button.classList.toggle("is-active", active);
      button.setAttribute("aria-selected", String(active));
    }

    applyRenderControlState();
    applyTextPreviewModeState();
    previewControls.onResult(format);
  };

  const setTextPreviewMode = (mode: TextPreviewMode): void => {
    stateStore.selectTextPreviewMode(mode);
    preview.setTextMode(mode);
    applyTextPreviewModeState();
  };

  const copyPreviewText = async (kind: PreviewCopyKind): Promise<void> => {
    const text = preview.getCopyText(kind);
    if (text === null) {
      updateShareStatus("Render a text preview before copying.");
      return;
    }

    const copied = await copyTextToClipboard(text);
    if (copied) {
      updateShareStatus(
        kind === "plain"
          ? "Plain text copied to clipboard."
          : "ANSI text copied to clipboard.",
      );
      return;
    }

    updateShareStatus(
      kind === "plain"
        ? "Clipboard access unavailable. Copy directly from the preview."
        : "Clipboard access unavailable. Switch to ANSI mode and copy directly from the preview.",
    );
  };

  const persistCurrentState = (): void => {
    const currentState = stateStore.getState();
    persistPlaygroundState(stateStorage, {
      input: currentState.input,
      format: currentState.format,
      renderSettings: currentState.renderSettings,
      textPreviewMode: currentState.textPreviewMode,
      selectedExampleId: currentState.selectedExampleId,
      customInput: currentState.customInput,
    });
  };

  let scheduleRender: (inputOverride?: string) => void = () => {};
  const snippetGallery = createSnippetGalleryController({
    root: snippetGrid,
    onCopySnippet: (snippet) => {
      void copyTextToClipboard(snippet.input).then((copied) => {
        if (copied) {
          updateShareStatus(`Copied snippet: ${snippet.name}.`);
          return;
        }
        updateShareStatus(
          "Clipboard access unavailable. Copy directly from the snippet preview.",
        );
      });
    },
    onRunSnippet: (snippet) => {
      if (stateStore.getState().selectedExampleId === DRAFT_EXAMPLE_ID) {
        stateStore.setCustomInput(editor.getValue());
      }

      previewControls.fitOnNextSvg();
      stateStore.selectExample(snippet.id);
      exampleSelect.value = snippet.id;
      setEditorValueProgrammatically(snippet.input);
      clearEditorStatus();
      persistCurrentState();
      scheduleRender(snippet.input);
      scrollWorkspaceIntoView(workspace);
      updateEditorStatus(`Loaded snippet in editor: ${snippet.name}.`);
    },
  });
  snippetGallery.render();

  if (!workerClient) {
    preview.showError("Web Worker support is unavailable in this environment.");
    previewControls.onResult("text");
    return;
  }

  const liveUpdate = createLiveUpdateController({
    debounceMs:
      options.debounceMs ??
      ((request) => defaultAdaptiveDebounce(request.input)),
    render: (request) => workerClient.render(request),
    onResult: (response) => {
      preview.showResult({
        format: response.format,
        output: response.output,
      });
      previewControls.onResult(response.format);
    },
    onError: (message) => {
      preview.showError(message);
      previewControls.onResult("text");
    },
  });

  scheduleRender = (inputOverride?: string): void => {
    const currentState = stateStore.getState();
    liveUpdate.schedule({
      input: inputOverride ?? currentState.input,
      format: currentState.format,
      configJson: currentConfigJson(),
    });
  };

  const setEditorValueProgrammatically = (value: string): void => {
    editor.setValue(value);
    stateStore.setInput(value);
  };

  for (const button of formatButtons) {
    button.addEventListener("click", () => {
      const format = button.dataset.format;
      if (!format || !isPlaygroundFormat(format)) {
        return;
      }

      setFormat(format);
      persistCurrentState();
      scheduleRender();
    });
  }

  advancedToggleButton.addEventListener("click", () => {
    setAdvancedPanelOpen(!stateStore.getState().advancedOpen);
  });

  for (const button of textPreviewModeButtons) {
    button.addEventListener("click", () => {
      const mode = button.dataset.textPreviewMode;
      if (!mode || !isTextPreviewMode(mode)) {
        return;
      }

      setTextPreviewMode(mode);
      persistCurrentState();
    });
  }

  copyPlainButton.addEventListener("click", () => {
    void copyPreviewText("plain");
  });

  copyAnsiButton.addEventListener("click", () => {
    void copyPreviewText("ansi");
  });

  exampleSelect.addEventListener("change", () => {
    const nextSelection = exampleSelect.value;
    if (nextSelection === DRAFT_EXAMPLE_ID) {
      stateStore.selectExample(DRAFT_EXAMPLE_ID);
      setEditorValueProgrammatically(stateStore.getState().customInput);
      clearEditorStatus();
      persistCurrentState();
      scheduleRender(stateStore.getState().customInput);
      return;
    }

    const nextExample = findExampleById(nextSelection);
    if (!nextExample) {
      exampleSelect.value = stateStore.getState().selectedExampleId;
      return;
    }

    if (stateStore.getState().selectedExampleId === DRAFT_EXAMPLE_ID) {
      stateStore.setCustomInput(editor.getValue());
    }
    stateStore.selectExample(nextExample.id);
    setEditorValueProgrammatically(nextExample.input);
    clearEditorStatus();
    persistCurrentState();
    scheduleRender(nextExample.input);
  });

  layoutEngineSelect.addEventListener("change", () => {
    if (isLayoutEngine(layoutEngineSelect.value)) {
      stateStore.updateRenderSettings({
        layoutEngine: layoutEngineSelect.value,
      });
      persistCurrentState();
      scheduleRender();
    }
  });

  edgePresetSelect.addEventListener("change", () => {
    if (isEdgePreset(edgePresetSelect.value)) {
      stateStore.updateRenderSettings({
        edgePreset: edgePresetSelect.value,
      });
      persistCurrentState();
      scheduleRender();
    }
  });

  pathSimplificationSelect.addEventListener("change", () => {
    if (isPathSimplification(pathSimplificationSelect.value)) {
      stateStore.updateRenderSettings({
        pathSimplification: pathSimplificationSelect.value,
      });
      persistCurrentState();
      scheduleRender();
    }
  });

  themeToggleButton.addEventListener("click", () => {
    const nextPreference = nextThemePreference(themeController.getPreference());
    themeController.setPreference(nextPreference);
    updateThemeToggleButton(themeToggleButton, themeController.getPreference());
  });

  shareButton.addEventListener("click", () => {
    const currentState = stateStore.getState();
    void shareStateService
      .copyShareUrl({
        input: currentState.input,
        format: currentState.format,
        renderSettings: currentState.renderSettings,
        textPreviewMode: currentState.textPreviewMode,
      })
      .then((copied) => {
        if (copied) {
          updateShareStatus("Share URL copied to clipboard.");
          return;
        }
        updateShareStatus("Share URL updated in address bar.");
      });
  });

  editor.onChange((value) => {
    clearEditorStatus();
    stateStore.setInput(value);
    syncSelectionOnEditorInput(value);
    persistCurrentState();
    scheduleRender(value);
  });

  applyRenderSettingsToControls();
  setAdvancedPanelOpen(false);
  setTextPreviewMode(stateStore.getState().textPreviewMode);
  setFormat(stateStore.getState().format);
  persistCurrentState();
  scheduleRender();
}

export function isBenchmarkModeEnabled(
  locationValue: SearchLocation = window.location,
): boolean {
  const params = new URLSearchParams(locationValue.search);
  const rawValue = params.get("benchmark");
  if (rawValue === null) {
    return false;
  }

  const normalized = rawValue.trim().toLowerCase();
  return normalized === "" || normalized === "1" || normalized === "true";
}

export async function bootstrapPlaygroundApp(
  root: HTMLElement,
  options: RenderAppOptions = {},
): Promise<void> {
  if (isBenchmarkModeEnabled(window.location)) {
    const { renderBenchmarkApp } = await import("../benchmark");
    await renderBenchmarkApp(root);
    return;
  }

  renderApp(root, options);
}
