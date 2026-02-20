import {
  HighlightStyle,
  StringStream,
  StreamLanguage,
  type StreamParser,
  syntaxHighlighting,
} from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { tags } from "@lezer/highlight";

type DiagramMode =
  | "flowchart"
  | "sequenceDiagram"
  | "classDiagram"
  | "stateDiagram"
  | "erDiagram"
  | "journey"
  | "gantt"
  | "requirementDiagram"
  | "gitGraph"
  | "pie"
  | "c4Diagram"
  | "info";

type MermaidMode = DiagramMode | "root";

interface MermaidTokenState {
  mode: MermaidMode;
  inConfigDirective: boolean;
}

interface DiagramKeywordConfig {
  typeKeywords: readonly string[];
  blockKeywords: readonly string[];
  keywords: readonly string[];
}

const DIAGRAM_KEYWORDS: Readonly<Record<DiagramMode, DiagramKeywordConfig>> = {
  flowchart: {
    typeKeywords: ["flowchart", "flowchart-v2", "graph"],
    blockKeywords: ["subgraph", "end"],
    keywords: [
      "TB",
      "TD",
      "BT",
      "RL",
      "LR",
      "click",
      "call",
      "href",
      "_self",
      "_blank",
      "_parent",
      "_top",
      "linkStyle",
      "style",
      "classDef",
      "class",
      "direction",
      "interpolate",
    ],
  },
  sequenceDiagram: {
    typeKeywords: ["sequenceDiagram"],
    blockKeywords: ["alt", "par", "and", "loop", "else", "end", "rect", "opt"],
    keywords: [
      "participant",
      "as",
      "Note",
      "note",
      "right of",
      "left of",
      "over",
      "activate",
      "deactivate",
      "autonumber",
      "title",
      "actor",
      "accDescription",
      "link",
      "links",
      "properties",
    ],
  },
  classDiagram: {
    typeKeywords: ["classDiagram", "classDiagram-v2"],
    blockKeywords: ["class"],
    keywords: [
      "link",
      "click",
      "callback",
      "call",
      "href",
      "cssClass",
      "direction",
      "TB",
      "BT",
      "RL",
      "LR",
      "title",
      "accDescription",
      "order",
    ],
  },
  stateDiagram: {
    typeKeywords: ["stateDiagram", "stateDiagram-v2"],
    blockKeywords: ["state", "note", "end"],
    keywords: [
      "as",
      "hide empty description",
      "direction",
      "TB",
      "BT",
      "RL",
      "LR",
    ],
  },
  erDiagram: {
    typeKeywords: ["erDiagram", "er"],
    blockKeywords: [],
    keywords: ["title", "accDescription"],
  },
  journey: {
    typeKeywords: ["journey"],
    blockKeywords: ["section"],
    keywords: ["title"],
  },
  gantt: {
    typeKeywords: ["gantt"],
    blockKeywords: ["section"],
    keywords: [
      "title",
      "dateFormat",
      "axisFormat",
      "todayMarker",
      "excludes",
      "inclusiveEndDates",
    ],
  },
  requirementDiagram: {
    typeKeywords: ["requirement", "requirementDiagram"],
    blockKeywords: [
      "requirement",
      "functionalRequirement",
      "interfaceRequirement",
      "performanceRequirement",
      "physicalRequirement",
      "designConstraint",
      "element",
    ],
    keywords: [],
  },
  gitGraph: {
    typeKeywords: ["gitGraph"],
    blockKeywords: [],
    keywords: [
      "accTitle",
      "accDescr",
      "commit",
      "cherry-pick",
      "branch",
      "merge",
      "reset",
      "checkout",
      "LR",
      "BT",
      "id",
      "msg",
      "type",
      "tag",
      "NORMAL",
      "REVERSE",
      "HIGHLIGHT",
    ],
  },
  pie: {
    typeKeywords: ["pie"],
    blockKeywords: [],
    keywords: ["title", "showData", "accDescription"],
  },
  c4Diagram: {
    typeKeywords: [
      "C4Context",
      "C4Container",
      "C4Component",
      "C4Dynamic",
      "C4Deployment",
    ],
    blockKeywords: [
      "Boundary",
      "Enterprise_Boundary",
      "System_Boundary",
      "Container_Boundary",
      "Node",
      "Node_L",
      "Node_R",
    ],
    keywords: [
      "title",
      "accDescription",
      "direction",
      "TB",
      "BT",
      "RL",
      "LR",
      "Person_Ext",
      "Person",
      "SystemQueue_Ext",
      "SystemDb_Ext",
      "System_Ext",
      "SystemQueue",
      "SystemDb",
      "System",
      "ContainerQueue_Ext",
      "ContainerDb_Ext",
      "Container_Ext",
      "ContainerQueue",
      "ContainerDb",
      "Container",
      "ComponentQueue_Ext",
      "ComponentDb_Ext",
      "Component_Ext",
      "ComponentQueue",
      "ComponentDb",
      "Component",
      "Deployment_Node",
      "Rel",
      "BiRel",
      "Rel_Up",
      "Rel_U",
      "Rel_Down",
      "Rel_D",
      "Rel_Left",
      "Rel_L",
      "Rel_Right",
      "Rel_R",
      "Rel_Back",
      "RelIndex",
    ],
  },
  info: {
    typeKeywords: ["info"],
    blockKeywords: [],
    keywords: ["showInfo"],
  },
};

interface ModeLookup {
  typeKeywordSet: Set<string>;
  blockKeywordSet: Set<string>;
  keywordSet: Set<string>;
  blockPhraseRegexes: RegExp[];
  keywordPhraseRegexes: RegExp[];
}

const MODE_LOOKUPS = Object.fromEntries(
  Object.entries(DIAGRAM_KEYWORDS).map(([mode, config]) => [
    mode,
    {
      typeKeywordSet: new Set(config.typeKeywords),
      blockKeywordSet: new Set(config.blockKeywords),
      keywordSet: new Set(config.keywords),
      blockPhraseRegexes: toPhraseRegexes(config.blockKeywords),
      keywordPhraseRegexes: toPhraseRegexes(config.keywords),
    },
  ]),
) as Record<DiagramMode, ModeLookup>;

const MODE_HEADERS: ReadonlyArray<{ mode: DiagramMode; regex: RegExp }> = [
  { mode: "gitGraph", regex: /^\s*gitGraph\b/ },
  { mode: "info", regex: /^\s*info\b/ },
  { mode: "pie", regex: /^\s*pie\b/ },
  { mode: "flowchart", regex: /^\s*(flowchart|flowchart-v2|graph)\b/ },
  { mode: "sequenceDiagram", regex: /^\s*sequenceDiagram\b/ },
  { mode: "classDiagram", regex: /^\s*classDiagram(?:-v2)?\b/ },
  { mode: "journey", regex: /^\s*journey\b/ },
  { mode: "gantt", regex: /^\s*gantt\b/ },
  { mode: "stateDiagram", regex: /^\s*stateDiagram(?:-v2)?\b/ },
  { mode: "erDiagram", regex: /^\s*er(?:Diagram)?\b/ },
  { mode: "requirementDiagram", regex: /^\s*requirement(?:Diagram)?\b/ },
  {
    mode: "c4Diagram",
    regex: /^\s*(C4Context|C4Container|C4Component|C4Dynamic|C4Deployment)\b/,
  },
];

const TRANSITION_REGEXES: ReadonlyArray<RegExp> = [
  /^[|}][o|](--|\.\.)[o|][{|]/,
  /^[ox]?(--+|==+)[ox]/,
  /^(--?>?>|--?[)x])[+-]?/,
  /^(<)?(--+|==+)(>)?/,
  /^:::/,
  /^-\.+->?/,
  /^->|^<-/,
];

const SHAPE_REGEXES: ReadonlyArray<RegExp> = [
  /^\[[^\]\n]*\]/,
  /^{[^}\n]*}/,
  /^\([^)\n]*\)/,
];

const INLINE_STRING_REGEXES: ReadonlyArray<RegExp> = [
  /^"(?:[^"\\]|\\.)*"/,
  /^`[^`\n]*`/,
  /^\|[^|\n]+\|/,
];

const IDENTIFIER_REGEX = /^[A-Za-z_][\w-]*/;
const NUMBER_REGEX = /^\d+(?:\.\d+)?/;
const ANNOTATION_REGEX = /^<<[^>\n]+>>/;
const DELIMITER_REGEX = /^[&;,()[\]{}:+~*$]/;

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function toPhraseRegexes(values: readonly string[]): RegExp[] {
  return values
    .filter((value) => value.includes(" "))
    .sort((left, right) => right.length - left.length)
    .map((value) => new RegExp(`^${escapeRegExp(value)}(?![\\w-])`));
}

function matchAnyRegex(
  stream: StringStream,
  regexes: readonly RegExp[],
): boolean {
  for (const regex of regexes) {
    if (stream.match(regex)) {
      return true;
    }
  }
  return false;
}

function detectModeAtLineStart(stream: StringStream): DiagramMode | null {
  for (const candidate of MODE_HEADERS) {
    if (stream.match(candidate.regex, false)) {
      stream.match(candidate.regex);
      return candidate.mode;
    }
  }

  return null;
}

function classifyIdentifier(word: string, mode: MermaidMode): string {
  if (mode === "root") {
    return "variable";
  }

  const lookup = MODE_LOOKUPS[mode];
  if (lookup.typeKeywordSet.has(word) || lookup.blockKeywordSet.has(word)) {
    return "atom";
  }
  if (lookup.keywordSet.has(word)) {
    return "keyword";
  }
  if (mode === "classDiagram" && /^[A-Z][\w-]*$/.test(word)) {
    return "def";
  }

  return "variable";
}

const mermaidStreamParser: StreamParser<MermaidTokenState> = {
  startState: () => ({
    mode: "root",
    inConfigDirective: false,
  }),
  copyState: (state) => ({
    mode: state.mode,
    inConfigDirective: state.inConfigDirective,
  }),
  token(stream, state) {
    if (stream.sol()) {
      const line = stream.string.trim();
      if (line.startsWith("%%{")) {
        stream.skipToEnd();
        state.inConfigDirective = !line.includes("}%%");
        return "string";
      }

      if (!line.startsWith("%%")) {
        const detectedMode = detectModeAtLineStart(stream);
        if (detectedMode) {
          state.mode = detectedMode;
          return "atom";
        }
      }
    }

    if (state.inConfigDirective) {
      stream.skipToEnd();
      if (/\}\s*%%\s*$/.test(stream.current())) {
        state.inConfigDirective = false;
      }
      return "string";
    }

    if (stream.eatSpace()) {
      return null;
    }

    if (stream.match(/^%%[^\n\r]*/)) {
      return "comment";
    }

    if (matchAnyRegex(stream, INLINE_STRING_REGEXES)) {
      return "string";
    }

    if (stream.match(ANNOTATION_REGEX)) {
      return "def";
    }

    if (matchAnyRegex(stream, TRANSITION_REGEXES)) {
      return "operator";
    }

    if (matchAnyRegex(stream, SHAPE_REGEXES)) {
      return "string";
    }

    if (stream.match(NUMBER_REGEX)) {
      return "number";
    }

    if (stream.match(DELIMITER_REGEX)) {
      return "punctuation";
    }

    if (state.mode !== "root") {
      const lookup = MODE_LOOKUPS[state.mode];
      if (
        matchAnyRegex(stream, lookup.blockPhraseRegexes) ||
        matchAnyRegex(stream, lookup.keywordPhraseRegexes)
      ) {
        return "keyword";
      }
    }

    if (stream.match(IDENTIFIER_REGEX)) {
      return classifyIdentifier(stream.current(), state.mode);
    }

    stream.next();
    return null;
  },
};

export const mermaidLanguage = StreamLanguage.define(mermaidStreamParser);

export const mermaidHighlightStyle = HighlightStyle.define([
  { tag: tags.atom, color: "var(--editor-token-type)", fontWeight: "700" },
  {
    tag: tags.keyword,
    color: "var(--editor-token-keyword)",
    fontWeight: "600",
  },
  {
    tag: tags.comment,
    color: "var(--editor-token-comment)",
    fontStyle: "italic",
  },
  { tag: tags.string, color: "var(--editor-token-string)" },
  { tag: tags.number, color: "var(--editor-token-number)" },
  { tag: tags.variableName, color: "var(--editor-token-variable)" },
  { tag: [tags.typeName, tags.className], color: "var(--editor-token-class)" },
  {
    tag: tags.operator,
    color: "var(--editor-token-transition)",
    fontWeight: "700",
  },
  {
    tag: [tags.punctuation, tags.separator],
    color: "var(--editor-token-delimiter)",
  },
]);

export const mermaidSyntaxHighlighting: Extension[] = [
  mermaidLanguage,
  syntaxHighlighting(mermaidHighlightStyle),
];

export interface MermaidTokenSpan {
  token: string | null;
  text: string;
}

export type MermaidTokenLine = MermaidTokenSpan[];

function advanceOneCharacter(stream: StringStream): void {
  if (stream.eol()) {
    return;
  }
  stream.next();
}

export function tokenizeMermaidText(input: string): MermaidTokenLine[] {
  const startState = mermaidStreamParser.startState;
  if (!startState) {
    return [];
  }

  const state = startState(2);
  const lines = input.split("\n");
  const tokenLines: MermaidTokenLine[] = [];

  for (const line of lines) {
    const stream = new StringStream(line, 2, 2);
    const lineTokens: MermaidTokenLine = [];

    while (!stream.eol()) {
      stream.start = stream.pos;
      const token = mermaidStreamParser.token(stream, state);
      if (stream.pos === stream.start) {
        advanceOneCharacter(stream);
      }

      const segment = line.slice(stream.start, stream.pos);
      if (segment.length > 0) {
        lineTokens.push({
          token: token ?? null,
          text: segment,
        });
      }
    }

    tokenLines.push(lineTokens);
  }

  return tokenLines;
}
