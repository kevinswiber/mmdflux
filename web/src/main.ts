import "../styles/main.css";

import { createEditorController } from "./editor";
import {
  DEFAULT_EXAMPLE_ID,
  findExampleById,
  PLAYGROUND_EXAMPLES,
  type PlaygroundExample,
} from "./examples";
import {
  type PlaygroundFormat as CapabilityPlaygroundFormat,
  helpText,
  isSupported,
  type RenderControlId,
} from "./format-capabilities";
import {
  createLiveUpdateController,
  type LiveUpdateDebounceSetting,
} from "./live-update";
import { createPreviewController } from "./preview";
import { createPreviewControls } from "./preview-controls";
import {
  DEFAULT_SHARE_RENDER_SETTINGS,
  decodeShareState,
  encodeShareState,
  normalizeShareRenderSettings,
  type ShareEdgePreset,
  type ShareGeometryLevel,
  type ShareLayoutEngine,
  type SharePathDetail,
  type ShareRenderSettings,
} from "./share";
import { createThemeController, type ThemePreference } from "./theme";
import type {
  WorkerOutputFormat,
  WorkerRequestMessage,
  WorkerResponseMessage,
} from "./worker-protocol";

export interface RenderRequest {
  seq: number;
  input: string;
  format: WorkerOutputFormat;
  configJson?: string;
}

export interface RenderResponse {
  seq: number;
  format: WorkerOutputFormat;
  output: string;
}

interface PendingRequest {
  resolve: (response: RenderResponse) => void;
  reject: (error: Error) => void;
}

export interface RenderWorkerClient {
  render: (request: RenderRequest) => Promise<RenderResponse>;
  terminate: () => void;
}

type PlaygroundFormat = CapabilityPlaygroundFormat;
type StateStorage = Pick<Storage, "getItem" | "setItem">;
type ExampleCategory = PlaygroundExample["category"];

interface PersistedPlaygroundStateV1 {
  v: 1;
  input: string;
  format: PlaygroundFormat;
}

interface PersistedPlaygroundStateV2 {
  v: 2;
  input: string;
  format: PlaygroundFormat;
  renderSettings: ShareRenderSettings;
}

interface EffectivePlaygroundState {
  input: string;
  format: PlaygroundFormat;
  renderSettings: ShareRenderSettings;
}

type PersistedPlaygroundState =
  | PersistedPlaygroundStateV1
  | PersistedPlaygroundStateV2;

interface RenderControlBinding {
  control: RenderControlId;
  select: HTMLSelectElement;
  help: HTMLElement;
  container: HTMLElement;
}

const PLAYGROUND_STATE_STORAGE_KEY = "mmdflux-playground-state";
const CATEGORY_ORDER: ExampleCategory[] = ["flowchart", "class"];
const CATEGORY_LABELS: Record<ExampleCategory, string> = {
  flowchart: "Flowchart",
  class: "Class",
};
const SNIPPET_PREVIEW_LINES = 7;

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

function resolveStateStorage(
  explicitStorage?: StateStorage,
): StateStorage | undefined {
  if (isStorageLike(explicitStorage)) {
    return explicitStorage;
  }

  try {
    return isStorageLike(window.localStorage) ? window.localStorage : undefined;
  } catch {
    return undefined;
  }
}

function isStorageLike(value: unknown): value is StateStorage {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Pick<Storage, "getItem">).getItem === "function" &&
    typeof (value as Pick<Storage, "setItem">).setItem === "function"
  );
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
    value === "smoothstep" ||
    value === "bezier"
  );
}

function isGeometryLevel(value: string): value is ShareGeometryLevel {
  return value === "layout" || value === "routed";
}

function isPathDetail(value: string): value is SharePathDetail {
  return (
    value === "full" ||
    value === "compact" ||
    value === "simplified" ||
    value === "endpoints"
  );
}

function parsePersistedPlaygroundState(
  rawValue: string | null,
): EffectivePlaygroundState | null {
  if (!rawValue) {
    return null;
  }

  try {
    const parsed = JSON.parse(rawValue) as Partial<PersistedPlaygroundState>;
    if (parsed.v !== 1 && parsed.v !== 2) {
      return null;
    }
    if (typeof parsed.input !== "string") {
      return null;
    }
    if (
      typeof parsed.format !== "string" ||
      !isPlaygroundFormat(parsed.format)
    ) {
      return null;
    }

    const renderSettings =
      parsed.v === 2
        ? normalizeShareRenderSettings(parsed.renderSettings)
        : DEFAULT_SHARE_RENDER_SETTINGS;

    return {
      input: parsed.input,
      format: parsed.format,
      renderSettings,
    };
  } catch {
    return null;
  }
}

function readPersistedPlaygroundState(
  storage: StateStorage | undefined,
): EffectivePlaygroundState | null {
  if (!storage) {
    return null;
  }

  return parsePersistedPlaygroundState(
    storage.getItem(PLAYGROUND_STATE_STORAGE_KEY),
  );
}

function persistPlaygroundState(
  storage: StateStorage | undefined,
  state: EffectivePlaygroundState,
): void {
  if (!storage) {
    return;
  }

  const persisted: PersistedPlaygroundStateV2 = {
    v: 2,
    input: state.input,
    format: state.format,
    renderSettings: state.renderSettings,
  };
  storage.setItem(PLAYGROUND_STATE_STORAGE_KEY, JSON.stringify(persisted));
}

function createDefaultWorker(): Worker {
  return new Worker(new URL("./worker.ts", import.meta.url), {
    type: "module",
  });
}

function isPlaygroundFormat(value: string): value is PlaygroundFormat {
  return value === "text" || value === "svg" || value === "mmds";
}

function toMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
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

function formatThemeLabel(preference: ThemePreference): string {
  if (preference === "system") {
    return "Theme: System";
  }
  if (preference === "light") {
    return "Theme: Light";
  }
  return "Theme: Dark";
}

async function copyToClipboard(text: string): Promise<boolean> {
  if (!navigator.clipboard?.writeText) {
    return false;
  }

  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

function getSnippetPreview(input: string): string {
  const lines = input.trim().split("\n");
  const previewLines = lines.slice(0, SNIPPET_PREVIEW_LINES);
  if (lines.length > SNIPPET_PREVIEW_LINES) {
    previewLines.push("...");
  }
  return previewLines.join("\n");
}

function populateExampleSelect(select: HTMLSelectElement): void {
  select.replaceChildren();

  for (const category of CATEGORY_ORDER) {
    const group = document.createElement("optgroup");
    group.label = CATEGORY_LABELS[category];

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

function removeCustomExampleOption(select: HTMLSelectElement): void {
  const customOption = select.querySelector<HTMLOptionElement>(
    'option[value="__custom__"]',
  );
  customOption?.remove();
}

export function createRenderWorkerClient(
  worker: Worker = createDefaultWorker(),
): RenderWorkerClient {
  const pending = new Map<number, PendingRequest>();

  worker.onmessage = (event: MessageEvent<WorkerResponseMessage>) => {
    const response = event.data;
    const pendingRequest = pending.get(response.seq);
    if (!pendingRequest) {
      return;
    }

    pending.delete(response.seq);

    if (response.type === "result") {
      pendingRequest.resolve({
        seq: response.seq,
        format: response.format,
        output: response.output,
      });
      return;
    }

    pendingRequest.reject(new Error(response.error));
  };

  return {
    render: (request: RenderRequest) => {
      const currentSeq = request.seq;

      return new Promise<RenderResponse>((resolve, reject) => {
        const message: WorkerRequestMessage = {
          type: "render",
          seq: currentSeq,
          input: request.input,
          format: request.format,
          configJson: request.configJson ?? "{}",
        };

        pending.set(currentSeq, { resolve, reject });

        try {
          worker.postMessage(message);
        } catch (error) {
          pending.delete(currentSeq);
          reject(
            new Error(`failed to post render request: ${toMessage(error)}`),
          );
        }
      });
    },
    terminate: () => {
      worker.terminate();
      for (const request of pending.values()) {
        request.reject(new Error("render worker terminated"));
      }
      pending.clear();
    },
  };
}

export function renderApp(
  root: HTMLElement,
  options: RenderAppOptions = {},
): void {
  const stateStorage = resolveStateStorage(options.stateStorage);
  const restoredShareState = decodeShareState(window.location.hash);
  const restoredLocalState = readPersistedPlaygroundState(stateStorage);
  const defaultExample =
    findExampleById(DEFAULT_EXAMPLE_ID) ?? PLAYGROUND_EXAMPLES[0];
  const initialInput =
    restoredShareState?.input ??
    restoredLocalState?.input ??
    defaultExample?.input ??
    "";
  const initialFormat =
    restoredShareState?.format ?? restoredLocalState?.format ?? "text";
  const initialRenderSettings =
    restoredShareState?.renderSettings ??
    restoredLocalState?.renderSettings ??
    DEFAULT_SHARE_RENDER_SETTINGS;

  root.innerHTML = `
    <main class="playground playground-app">
      <header class="toolbar">
        <div class="toolbar-title-group">
          <h1>mmdflux playground <a href="https://github.com/kevinswiber/mmdflux" target="_blank" rel="noopener noreferrer" class="repo-link">kevinswiber/mmdflux</a></h1>
          <p class="toolbar-subtitle">Render Mermaid diagrams in text, SVG, and MMDS with instant feedback.</p>
        </div>
        <div class="toolbar-actions toolbar-actions-primary">
          <label class="example-picker">
            <span>Example</span>
            <select data-example-select></select>
          </label>
          <div class="format-tabs" role="tablist" aria-label="Output format">
            <button type="button" role="tab" data-format="text" aria-selected="true" class="is-active">Text</button>
            <button type="button" role="tab" data-format="svg" aria-selected="false">SVG</button>
            <button type="button" role="tab" data-format="mmds" aria-selected="false">MMDS</button>
          </div>
          <button type="button" class="toolbar-button" data-advanced-toggle aria-expanded="false">Advanced settings</button>
          <div class="export-control">
            <button type="button" class="toolbar-button" data-export-toggle hidden>Export</button>
            <div class="export-menu" data-export-menu hidden>
              <button type="button" data-export-svg>Download SVG</button>
              <button type="button" data-export-png>Download PNG</button>
            </div>
          </div>
          <button type="button" class="toolbar-button" data-theme-toggle>Theme: System</button>
          <button type="button" class="toolbar-button" data-share>Copy Share URL</button>
        </div>
      </header>

      <section class="advanced-panel" data-advanced-panel hidden>
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
              <option value="smoothstep">smoothstep</option>
              <option value="bezier">bezier</option>
            </select>
            <p class="render-help" data-help-edge-preset></p>
          </div>
          <div class="render-setting" data-setting="geometryLevel">
            <label for="geometry-level-select">Geometry Level</label>
            <select id="geometry-level-select" data-geometry-level>
              <option value="layout">layout</option>
              <option value="routed">routed</option>
            </select>
            <p class="render-help" data-help-geometry-level></p>
          </div>
          <div class="render-setting" data-setting="pathDetail">
            <label for="path-detail-select">Path Detail</label>
            <select id="path-detail-select" data-path-detail>
              <option value="full">full</option>
              <option value="compact">compact</option>
              <option value="simplified">simplified</option>
              <option value="endpoints">endpoints</option>
            </select>
            <p class="render-help" data-help-path-detail></p>
          </div>
        </div>
      </section>

      <section class="workspace">
        <div class="panel">
          <h2>Input</h2>
          <div data-editor-root></div>
        </div>
        <div class="panel">
          <h2>Preview</h2>
          <p class="share-status" data-share-status hidden></p>
          <p class="preview-error" data-preview-error hidden></p>
          <div class="preview-toolbar" data-preview-controls hidden>
            <button type="button" class="preview-toolbar-button" data-zoom-out>-</button>
            <span class="preview-zoom-label" data-zoom-label>100%</span>
            <button type="button" class="preview-toolbar-button" data-zoom-in>+</button>
            <button type="button" class="preview-toolbar-button" data-zoom-fit>Fit</button>
            <button type="button" class="preview-toolbar-button" data-zoom-reset>Reset</button>
          </div>
          <div class="preview-output" data-preview-output></div>
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
  const previewOutput = root.querySelector<HTMLElement>(
    "[data-preview-output]",
  );
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
  const snippetGrid = root.querySelector<HTMLElement>("[data-snippet-grid]");

  const layoutEngineSelect = root.querySelector<HTMLSelectElement>(
    "[data-layout-engine]",
  );
  const edgePresetSelect =
    root.querySelector<HTMLSelectElement>("[data-edge-preset]");
  const geometryLevelSelect = root.querySelector<HTMLSelectElement>(
    "[data-geometry-level]",
  );
  const pathDetailSelect =
    root.querySelector<HTMLSelectElement>("[data-path-detail]");

  const layoutHelp = root.querySelector<HTMLElement>(
    "[data-help-layout-engine]",
  );
  const edgeHelp = root.querySelector<HTMLElement>("[data-help-edge-preset]");
  const geometryHelp = root.querySelector<HTMLElement>(
    "[data-help-geometry-level]",
  );
  const pathHelp = root.querySelector<HTMLElement>("[data-help-path-detail]");

  const layoutSetting = root.querySelector<HTMLElement>(
    '[data-setting="layoutEngine"]',
  );
  const edgeSetting = root.querySelector<HTMLElement>(
    '[data-setting="edgePreset"]',
  );
  const geometrySetting = root.querySelector<HTMLElement>(
    '[data-setting="geometryLevel"]',
  );
  const pathSetting = root.querySelector<HTMLElement>(
    '[data-setting="pathDetail"]',
  );

  const previewControlsRoot = root.querySelector<HTMLElement>(
    "[data-preview-controls]",
  );
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
    !previewOutput ||
    !previewError ||
    !shareStatus ||
    !shareButton ||
    !themeToggleButton ||
    !exampleSelect ||
    !advancedToggleButton ||
    !advancedPanel ||
    !snippetGrid ||
    !layoutEngineSelect ||
    !edgePresetSelect ||
    !geometryLevelSelect ||
    !pathDetailSelect ||
    !layoutHelp ||
    !edgeHelp ||
    !geometryHelp ||
    !pathHelp ||
    !layoutSetting ||
    !edgeSetting ||
    !geometrySetting ||
    !pathSetting ||
    !previewControlsRoot ||
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
    initialValue: initialInput,
  });
  const previewControls = createPreviewControls({
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

  const renderSnippetCards = (): void => {
    snippetGrid.replaceChildren();

    const orderedExamples = [...PLAYGROUND_EXAMPLES].sort((left, right) => {
      if (left.featured !== right.featured) {
        return left.featured ? -1 : 1;
      }
      if (left.category !== right.category) {
        return (
          CATEGORY_ORDER.indexOf(left.category) -
          CATEGORY_ORDER.indexOf(right.category)
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
      badge.textContent = CATEGORY_LABELS[example.category];

      header.append(title, badge);

      const description = document.createElement("p");
      description.className = "snippet-description";
      description.textContent = example.description;

      const previewBlock = document.createElement("pre");
      previewBlock.className = "snippet-preview";
      previewBlock.textContent = getSnippetPreview(example.input);

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
      snippetGrid.append(card);
    }
  };

  renderSnippetCards();

  const matchedExample = PLAYGROUND_EXAMPLES.find(
    (example) => example.input === initialInput,
  );
  if (matchedExample) {
    exampleSelect.value = matchedExample.id;
  } else {
    const customOption = document.createElement("option");
    customOption.value = "__custom__";
    customOption.textContent = "Custom from URL";
    customOption.dataset.custom = "true";
    exampleSelect.prepend(customOption);
    exampleSelect.value = "__custom__";
  }

  const matchMedia =
    typeof window.matchMedia === "function"
      ? window.matchMedia.bind(window)
      : undefined;
  const themeStorage = (() => {
    try {
      return isStorageLike(window.localStorage)
        ? window.localStorage
        : undefined;
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
  themeToggleButton.textContent = formatThemeLabel(
    themeController.getPreference(),
  );

  let selectedFormat: PlaygroundFormat = initialFormat;
  let renderSettings: ShareRenderSettings = normalizeShareRenderSettings(
    initialRenderSettings,
  );
  let advancedOpen = false;

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
      control: "geometryLevel",
      select: geometryLevelSelect,
      help: geometryHelp,
      container: geometrySetting,
    },
    {
      control: "pathDetail",
      select: pathDetailSelect,
      help: pathHelp,
      container: pathSetting,
    },
  ];

  const updateShareStatus = (message: string): void => {
    shareStatus.hidden = false;
    shareStatus.textContent = message;
  };

  previewControls.setStatusReporter((message) => {
    updateShareStatus(message);
  });

  const applyRenderSettingsToControls = (): void => {
    layoutEngineSelect.value = renderSettings.layoutEngine;
    edgePresetSelect.value = renderSettings.edgePreset;
    geometryLevelSelect.value = renderSettings.geometryLevel;
    pathDetailSelect.value = renderSettings.pathDetail;
  };

  const applyRenderControlState = (): void => {
    for (const binding of renderControlBindings) {
      const supported = isSupported(selectedFormat, binding.control);
      binding.select.disabled = !supported;
      binding.help.textContent = helpText(selectedFormat, binding.control);
      binding.container.classList.toggle("is-disabled", !supported);
    }
  };

  const currentConfigJson = (): string => {
    const config: Record<string, string> = {};
    if (renderSettings.layoutEngine !== "auto") {
      config.layoutEngine = renderSettings.layoutEngine;
    }

    if (selectedFormat === "svg") {
      if (renderSettings.edgePreset !== "auto") {
        config.edgePreset = renderSettings.edgePreset;
      }
      if (renderSettings.pathDetail !== "full") {
        config.pathDetail = renderSettings.pathDetail;
      }
    }

    if (selectedFormat === "mmds") {
      config.geometryLevel = renderSettings.geometryLevel;
      if (renderSettings.pathDetail !== "full") {
        config.pathDetail = renderSettings.pathDetail;
      }
    }

    return JSON.stringify(config);
  };

  const setAdvancedPanelOpen = (open: boolean): void => {
    advancedOpen = open;
    advancedPanel.hidden = !open;
    advancedToggleButton.setAttribute("aria-expanded", String(open));
    advancedToggleButton.classList.toggle("is-active", open);
    advancedToggleButton.textContent = open
      ? "Hide Advanced settings"
      : "Advanced settings";
  };

  const setFormat = (format: PlaygroundFormat): void => {
    selectedFormat = format;
    for (const button of formatButtons) {
      const active = button.dataset.format === format;
      button.classList.toggle("is-active", active);
      button.setAttribute("aria-selected", String(active));
    }

    applyRenderControlState();
    previewControls.onResult(format);
  };

  const persistCurrentState = (): void => {
    persistPlaygroundState(stateStorage, {
      input: editor.getValue(),
      format: selectedFormat,
      renderSettings,
    });
  };

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

  const scheduleRender = (): void => {
    liveUpdate.schedule({
      input: editor.getValue(),
      format: selectedFormat,
      configJson: currentConfigJson(),
    });
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
    setAdvancedPanelOpen(!advancedOpen);
  });

  snippetGrid.addEventListener("click", (event) => {
    const target = event.target;
    if (!(target instanceof Element)) {
      return;
    }

    const copyButton = target.closest<HTMLButtonElement>("[data-snippet-copy]");
    if (copyButton) {
      const snippet = findExampleById(copyButton.dataset.snippetCopy ?? "");
      if (!snippet) {
        return;
      }

      void copyToClipboard(snippet.input).then((copied) => {
        if (copied) {
          updateShareStatus(`Copied snippet: ${snippet.name}.`);
          return;
        }
        updateShareStatus(
          "Clipboard access unavailable. Copy directly from the snippet preview.",
        );
      });
      return;
    }

    const runButton = target.closest<HTMLButtonElement>("[data-snippet-run]");
    if (!runButton) {
      return;
    }

    const snippet = findExampleById(runButton.dataset.snippetRun ?? "");
    if (!snippet) {
      return;
    }

    removeCustomExampleOption(exampleSelect);
    exampleSelect.value = snippet.id;
    editor.setValue(snippet.input);
    persistCurrentState();
    scheduleRender();
    updateShareStatus(`Loaded snippet in editor: ${snippet.name}.`);
  });

  exampleSelect.addEventListener("change", () => {
    const nextExample = findExampleById(exampleSelect.value);
    if (!nextExample) {
      return;
    }

    editor.setValue(nextExample.input);
    persistCurrentState();
    scheduleRender();
  });

  layoutEngineSelect.addEventListener("change", () => {
    if (isLayoutEngine(layoutEngineSelect.value)) {
      renderSettings = {
        ...renderSettings,
        layoutEngine: layoutEngineSelect.value,
      };
      persistCurrentState();
      scheduleRender();
    }
  });

  edgePresetSelect.addEventListener("change", () => {
    if (isEdgePreset(edgePresetSelect.value)) {
      renderSettings = {
        ...renderSettings,
        edgePreset: edgePresetSelect.value,
      };
      persistCurrentState();
      scheduleRender();
    }
  });

  geometryLevelSelect.addEventListener("change", () => {
    if (isGeometryLevel(geometryLevelSelect.value)) {
      renderSettings = {
        ...renderSettings,
        geometryLevel: geometryLevelSelect.value,
      };
      persistCurrentState();
      scheduleRender();
    }
  });

  pathDetailSelect.addEventListener("change", () => {
    if (isPathDetail(pathDetailSelect.value)) {
      renderSettings = {
        ...renderSettings,
        pathDetail: pathDetailSelect.value,
      };
      persistCurrentState();
      scheduleRender();
    }
  });

  themeToggleButton.addEventListener("click", () => {
    const nextPreference = nextThemePreference(themeController.getPreference());
    themeController.setPreference(nextPreference);
    themeToggleButton.textContent = formatThemeLabel(
      themeController.getPreference(),
    );
  });

  shareButton.addEventListener("click", () => {
    const shareState = {
      input: editor.getValue(),
      format: selectedFormat,
      renderSettings,
    };
    const hash = encodeShareState(shareState);
    const shareUrl = `${window.location.origin}${window.location.pathname}#${hash}`;

    history.replaceState(null, "", `#${hash}`);

    void copyToClipboard(shareUrl).then((copied) => {
      if (copied) {
        updateShareStatus("Share URL copied to clipboard.");
        return;
      }
      updateShareStatus("Share URL updated in address bar.");
    });
  });

  editor.onChange(() => {
    persistCurrentState();
    scheduleRender();
  });

  applyRenderSettingsToControls();
  setAdvancedPanelOpen(false);
  setFormat(selectedFormat);
  persistCurrentState();
  scheduleRender();
}

function searchFromLocation(locationValue: SearchLocation): string {
  if (locationValue instanceof URL) {
    return locationValue.search;
  }

  return locationValue.search;
}

export function isBenchmarkModeEnabled(
  locationValue: SearchLocation = window.location,
): boolean {
  const params = new URLSearchParams(searchFromLocation(locationValue));
  const rawValue = params.get("benchmark");
  if (rawValue === null) {
    return false;
  }

  const normalized = rawValue.trim().toLowerCase();
  return normalized === "" || normalized === "1" || normalized === "true";
}

async function mountApp(root: HTMLElement): Promise<void> {
  if (isBenchmarkModeEnabled(window.location)) {
    const { renderBenchmarkApp } = await import("./benchmark");
    await renderBenchmarkApp(root);
    return;
  }

  renderApp(root);
}

const root = document.querySelector<HTMLElement>("#app");
if (root) {
  void mountApp(root);
}
