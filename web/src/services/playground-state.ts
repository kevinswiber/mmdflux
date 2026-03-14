import {
  DRAFT_EXAMPLE_ID,
  type ExampleSelectionId,
  isPlaygroundFormat,
  type PlaygroundStateSnapshot,
} from "../app/state";
import {
  DEFAULT_SHARE_RENDER_SETTINGS,
  normalizeShareRenderSettings,
  normalizeShareTextPreviewMode,
} from "../share";

export type StateStorage = Pick<Storage, "getItem" | "setItem">;

interface PersistedPlaygroundStateV1 {
  v: 1;
  input: string;
  format: string;
}

interface PersistedPlaygroundStateV2 {
  v: 2;
  input: string;
  format: string;
  renderSettings: unknown;
}

interface PersistedPlaygroundStateV3 {
  v: 3;
  input: string;
  format: string;
  renderSettings: unknown;
  selectedExampleId: string;
  customInput: string;
}

interface PersistedPlaygroundStateV4 {
  v: 4;
  input: string;
  format: string;
  renderSettings: unknown;
  textPreviewMode: string;
  selectedExampleId: string;
  customInput: string;
}

type PersistedPlaygroundState =
  | PersistedPlaygroundStateV1
  | PersistedPlaygroundStateV2
  | PersistedPlaygroundStateV3
  | PersistedPlaygroundStateV4;

interface ParsePersistedPlaygroundStateOptions {
  findExampleIdByInput?: (input: string) => string | null;
  isKnownExampleId?: (id: string) => boolean;
}

const PLAYGROUND_STATE_STORAGE_KEY = "mmdflux-playground-state";
const LEGACY_CUSTOM_EXAMPLE_ID = "__custom__";

export function isStorageLike(value: unknown): value is StateStorage {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Pick<Storage, "getItem">).getItem === "function" &&
    typeof (value as Pick<Storage, "setItem">).setItem === "function"
  );
}

export function resolveStateStorage(
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

export function parsePersistedPlaygroundState(
  rawValue: string | null,
  options: ParsePersistedPlaygroundStateOptions = {},
): PlaygroundStateSnapshot | null {
  if (!rawValue) {
    return null;
  }

  try {
    const parsed = JSON.parse(rawValue) as Partial<PersistedPlaygroundState>;
    if (parsed.v !== 1 && parsed.v !== 2 && parsed.v !== 3 && parsed.v !== 4) {
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

    const parsedRenderSettings =
      "renderSettings" in parsed ? parsed.renderSettings : undefined;
    const renderSettings =
      parsed.v === 1
        ? DEFAULT_SHARE_RENDER_SETTINGS
        : normalizeShareRenderSettings(parsedRenderSettings);

    const matchingExampleId =
      options.findExampleIdByInput?.(parsed.input) ?? null;
    const parsedSelectedExampleId =
      (parsed.v === 3 || parsed.v === 4) &&
      typeof parsed.selectedExampleId === "string"
        ? parsed.selectedExampleId === LEGACY_CUSTOM_EXAMPLE_ID
          ? DRAFT_EXAMPLE_ID
          : parsed.selectedExampleId
        : null;
    const selectedExampleId =
      parsedSelectedExampleId &&
      (parsedSelectedExampleId === DRAFT_EXAMPLE_ID ||
        options.isKnownExampleId?.(parsedSelectedExampleId))
        ? (parsedSelectedExampleId as ExampleSelectionId)
        : ((matchingExampleId ?? DRAFT_EXAMPLE_ID) as ExampleSelectionId);
    const customInput =
      (parsed.v === 3 || parsed.v === 4) &&
      typeof parsed.customInput === "string"
        ? parsed.customInput
        : parsed.input;
    const textPreviewMode =
      "textPreviewMode" in parsed
        ? normalizeShareTextPreviewMode(parsed.textPreviewMode)
        : "plain";

    return {
      input: parsed.input,
      format: parsed.format,
      renderSettings,
      textPreviewMode,
      selectedExampleId,
      customInput,
    };
  } catch {
    return null;
  }
}

export function readPersistedPlaygroundState(
  storage: StateStorage | undefined,
  options: ParsePersistedPlaygroundStateOptions = {},
): PlaygroundStateSnapshot | null {
  if (!storage) {
    return null;
  }

  return parsePersistedPlaygroundState(
    storage.getItem(PLAYGROUND_STATE_STORAGE_KEY),
    options,
  );
}

export function persistPlaygroundState(
  storage: StateStorage | undefined,
  state: PlaygroundStateSnapshot,
): void {
  if (!storage) {
    return;
  }

  storage.setItem(
    PLAYGROUND_STATE_STORAGE_KEY,
    JSON.stringify({
      v: 4,
      input: state.input,
      format: state.format,
      renderSettings: state.renderSettings,
      textPreviewMode: state.textPreviewMode,
      selectedExampleId: state.selectedExampleId,
      customInput: state.customInput,
    }),
  );
}
