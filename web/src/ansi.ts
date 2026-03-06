export interface AnsiTextStyle {
  foreground: string | null;
  background: string | null;
}

export interface AnsiSegment {
  text: string;
  style: AnsiTextStyle;
}

const ESCAPE = "\u001b";
const ANSI_SGR_SEQUENCE = new RegExp(`${ESCAPE}\\[[0-9;]*m`, "g");
const ANSI_SGR_CAPTURE_SEQUENCE = new RegExp(`${ESCAPE}\\[([0-9;]*)m`, "g");

export function stripAnsi(input: string): string {
  return input.replace(ANSI_SGR_SEQUENCE, "");
}

export function escapeAnsiForDisplay(input: string): string {
  return input.replaceAll("\u001b", "\\x1b");
}

export function parseAnsiSegments(input: string): AnsiSegment[] {
  const segments: AnsiSegment[] = [];
  let activeStyle: AnsiTextStyle = {
    foreground: null,
    background: null,
  };
  let lastIndex = 0;

  for (const match of input.matchAll(ANSI_SGR_CAPTURE_SEQUENCE)) {
    const matchIndex = match.index ?? 0;
    if (matchIndex > lastIndex) {
      pushSegment(segments, input.slice(lastIndex, matchIndex), activeStyle);
    }

    activeStyle = applySgrParameters(activeStyle, match[1] ?? "");
    lastIndex = matchIndex + match[0].length;
  }

  if (lastIndex < input.length) {
    pushSegment(segments, input.slice(lastIndex), activeStyle);
  }

  return segments;
}

function pushSegment(
  segments: AnsiSegment[],
  text: string,
  style: AnsiTextStyle,
): void {
  if (!text) {
    return;
  }

  const lastSegment = segments.at(-1);
  if (lastSegment && stylesEqual(lastSegment.style, style)) {
    lastSegment.text += text;
    return;
  }

  segments.push({
    text,
    style: {
      foreground: style.foreground,
      background: style.background,
    },
  });
}

function stylesEqual(left: AnsiTextStyle, right: AnsiTextStyle): boolean {
  return (
    left.foreground === right.foreground && left.background === right.background
  );
}

function applySgrParameters(
  style: AnsiTextStyle,
  rawParams: string,
): AnsiTextStyle {
  const nextStyle: AnsiTextStyle = {
    foreground: style.foreground,
    background: style.background,
  };
  const params =
    rawParams.trim() === ""
      ? [0]
      : rawParams
          .split(";")
          .map((value) => Number.parseInt(value, 10))
          .filter(Number.isFinite);

  for (let index = 0; index < params.length; index += 1) {
    const param = params[index];
    if (param === 0) {
      nextStyle.foreground = null;
      nextStyle.background = null;
      continue;
    }

    if (param === 39) {
      nextStyle.foreground = null;
      continue;
    }

    if (param === 49) {
      nextStyle.background = null;
      continue;
    }

    if (param === 38 || param === 48) {
      const parsed = parseTrueColor(params, index + 1);
      if (!parsed) {
        continue;
      }

      if (param === 38) {
        nextStyle.foreground = parsed.cssColor;
      } else {
        nextStyle.background = parsed.cssColor;
      }
      index += parsed.consumed;
    }
  }

  return nextStyle;
}

function parseTrueColor(
  params: number[],
  startIndex: number,
): { cssColor: string; consumed: number } | null {
  if (params[startIndex] !== 2 || startIndex + 3 >= params.length) {
    return null;
  }

  const [red, green, blue] = params.slice(startIndex + 1, startIndex + 4);
  if (![red, green, blue].every(Number.isFinite)) {
    return null;
  }

  return {
    cssColor: `rgb(${red}, ${green}, ${blue})`,
    consumed: 4,
  };
}
