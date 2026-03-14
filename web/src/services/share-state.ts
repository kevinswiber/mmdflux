import { decodeShareState, encodeShareState, type ShareState } from "../share";

interface ClipboardLike {
  writeText: (text: string) => Promise<void>;
}

interface ShareLocationLike {
  hash: string;
  origin: string;
  pathname: string;
}

interface ShareHistoryLike {
  replaceState: (
    data: unknown,
    unused: string,
    url?: string | URL | null,
  ) => void;
}

interface CreateShareStateServiceOptions {
  clipboard?: ClipboardLike;
  history?: ShareHistoryLike;
  location?: ShareLocationLike;
}

export interface ShareStateService {
  readState: () => ShareState | null;
  copyShareUrl: (state: ShareState) => Promise<boolean>;
}

export async function copyTextToClipboard(
  text: string,
  clipboard: ClipboardLike | undefined = navigator.clipboard,
): Promise<boolean> {
  if (!clipboard?.writeText) {
    return false;
  }

  try {
    await clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

export function createShareStateService(
  options: CreateShareStateServiceOptions = {},
): ShareStateService {
  const locationValue = options.location ?? window.location;
  const historyValue = options.history ?? window.history;

  return {
    readState: () => decodeShareState(locationValue.hash),
    copyShareUrl: async (state) => {
      const hash = encodeShareState(state);
      const shareUrl = `${locationValue.origin}${locationValue.pathname}#${hash}`;

      historyValue.replaceState(null, "", `#${hash}`);
      return copyTextToClipboard(shareUrl, options.clipboard);
    },
  };
}
