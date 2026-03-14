import type { PlaygroundExample } from "../examples";
import type { TextPreviewMode } from "../preview";
import {
  normalizeShareRenderSettings,
  normalizeShareTextPreviewMode,
  type ShareRenderSettings,
} from "../share";

export type PlaygroundFormat = "text" | "svg" | "mmds";

export const DRAFT_EXAMPLE_ID = "__draft__";

export type ExampleSelectionId =
  | PlaygroundExample["id"]
  | typeof DRAFT_EXAMPLE_ID;

export interface PlaygroundStateSnapshot {
  input: string;
  format: PlaygroundFormat;
  renderSettings: ShareRenderSettings;
  textPreviewMode: TextPreviewMode;
  selectedExampleId: ExampleSelectionId;
  customInput: string;
}

export interface PlaygroundState extends PlaygroundStateSnapshot {
  advancedOpen: boolean;
}

export interface PlaygroundStateStore {
  getState: () => PlaygroundState;
  subscribe: (listener: (state: PlaygroundState) => void) => () => void;
  setInput: (input: string) => void;
  selectFormat: (format: PlaygroundFormat) => void;
  setRenderSettings: (settings: ShareRenderSettings) => void;
  updateRenderSettings: (patch: Partial<ShareRenderSettings>) => void;
  selectTextPreviewMode: (mode: TextPreviewMode) => void;
  selectExample: (id: ExampleSelectionId) => void;
  setCustomInput: (input: string) => void;
  setAdvancedOpen: (open: boolean) => void;
}

interface CreatePlaygroundStateStoreOptions {
  initialState?: Partial<PlaygroundState>;
}

export function isPlaygroundFormat(value: string): value is PlaygroundFormat {
  return value === "text" || value === "svg" || value === "mmds";
}

function normalizeInitialState(
  initialState: Partial<PlaygroundState> | undefined,
): PlaygroundState {
  const input = initialState?.input ?? "";
  return {
    input,
    format: initialState?.format ?? "svg",
    renderSettings: normalizeShareRenderSettings(initialState?.renderSettings),
    textPreviewMode: normalizeShareTextPreviewMode(
      initialState?.textPreviewMode,
    ),
    selectedExampleId: initialState?.selectedExampleId ?? DRAFT_EXAMPLE_ID,
    customInput: initialState?.customInput ?? input,
    advancedOpen: initialState?.advancedOpen ?? false,
  };
}

export function createPlaygroundStateStore(
  options: CreatePlaygroundStateStoreOptions = {},
): PlaygroundStateStore {
  let state = normalizeInitialState(options.initialState);
  const listeners = new Set<(state: PlaygroundState) => void>();

  const setState = (nextState: PlaygroundState): void => {
    state = nextState;
    for (const listener of listeners) {
      listener(state);
    }
  };

  return {
    getState: () => state,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    setInput: (input) => {
      setState({
        ...state,
        input,
      });
    },
    selectFormat: (format) => {
      setState({
        ...state,
        format,
      });
    },
    setRenderSettings: (renderSettings) => {
      setState({
        ...state,
        renderSettings: normalizeShareRenderSettings(renderSettings),
      });
    },
    updateRenderSettings: (patch) => {
      setState({
        ...state,
        renderSettings: normalizeShareRenderSettings({
          ...state.renderSettings,
          ...patch,
        }),
      });
    },
    selectTextPreviewMode: (textPreviewMode) => {
      setState({
        ...state,
        textPreviewMode: normalizeShareTextPreviewMode(textPreviewMode),
      });
    },
    selectExample: (selectedExampleId) => {
      setState({
        ...state,
        selectedExampleId,
      });
    },
    setCustomInput: (customInput) => {
      setState({
        ...state,
        customInput,
      });
    },
    setAdvancedOpen: (advancedOpen) => {
      setState({
        ...state,
        advancedOpen,
      });
    },
  };
}
