/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import { history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { yaml } from "@codemirror/lang-yaml";
import { linter, setDiagnostics, type Diagnostic } from "@codemirror/lint";
import { EditorState, type Extension } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  hoverTooltip,
  keymap,
  MatchDecorator,
  type Tooltip,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import {
  foldGutter,
  syntaxHighlighting,
  HighlightStyle,
} from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { parseDocument } from "yaml";
import {
  extractOutboundProfileNames,
  getOxiDnsConfigSubKeys,
  getOxiDnsConfigValueSuggestions,
  OXIDNS_LOG_LEVELS,
  type OxiDnsConfigValueSuggestion,
} from "@/lib/oxidns-config-schema";
import {
  getPluginKindDefinition,
  getLocalizedPluginKindDefinition,
  getLocalizedPluginKindDefinitions,
  pluginKindDefinitions,
  type ConfigField,
  type ConfigFieldChild,
} from "@/lib/plugin-definitions";
import type { PluginInstance, PluginType } from "@/lib/types";
import { DEFAULT_LOCALE, WEBUI, t as translate, type Locale } from "@/lib/i18n";
import { pluginTypeLabel } from "@/lib/i18n/plugin-defined";
import {
  pluginTagValidationMessageKey,
  validatePluginTag,
} from "@/lib/plugin-tags";

export type OxiDnsYamlEditorVariant =
  | "config"
  | "plugin-args"
  | "sequence"
  | "generic";

export interface OxiDnsYamlEditorContext {
  variant: OxiDnsYamlEditorVariant;
  locale?: Locale;
  plugins?: PluginInstance[];
  pluginKind?: string;
  fields?: ConfigField[];
  currentPluginName?: string;
  outboundProfileNames?: string[];
}

export interface OxiDnsYamlDiagnostic {
  message: string;
  severity?: "error" | "warning" | "info";
  line?: number;
  column?: number;
  end_line?: number;
  end_column?: number;
}

const sequenceControls = [
  "accept",
  "return",
  "reject",
  "mark",
  "set_mark",
  "jump",
  "goto",
];
const sequenceControlExamples = [
  "reject SERVFAIL",
  "reject servfail",
  "reject NOERROR",
  "reject 3",
  "mark 1,2",
  "set_mark 2,3",
];
const editorFontFamily =
  "JetBrains Mono, ui-monospace, SFMono-Regular, Menlo, Consolas, Liberation Mono, monospace";

export type OxiDnsYamlTheme = "dark" | "light";

interface OxiDnsYamlPalette {
  activeLine: string;
  activeLineGutter: string;
  border: string;
  boolean: string;
  comment: string;
  cursor: string;
  diffDeleteLine: string;
  diffDeleteMarker: string;
  diffDeleteText: string;
  diffInsertLine: string;
  diffInsertMarker: string;
  diffInsertText: string;
  foldHoverBackground: string;
  foldHoverForeground: string;
  foldMarker: string;
  foreground: string;
  gutterForeground: string;
  key: string;
  keyword: string;
  nullValue: string;
  number: string;
  punctuation: string;
  selection: string;
  string: string;
  tag: string;
  tooltipBackground: string;
  type: string;
}

const lightPalette: OxiDnsYamlPalette = {
  activeLine: "#eef6ff",
  activeLineGutter: "#4b5563",
  border: "#d8dce3",
  boolean: "#000080",
  comment: "#808080",
  cursor: "#0b6fcb",
  diffDeleteLine: "#fff1f0",
  diffDeleteMarker: "#d73a49",
  diffDeleteText: "#ffd7d5",
  diffInsertLine: "#ecfff1",
  diffInsertMarker: "#2da44e",
  diffInsertText: "#c9f5d4",
  foldHoverBackground: "#e8f2ff",
  foldHoverForeground: "#0b6fcb",
  foldMarker: "#7b8494",
  foreground: "#000000",
  gutterForeground: "#9aa0aa",
  key: "#660e7a",
  keyword: "#000080",
  nullValue: "#000080",
  number: "#0000ff",
  punctuation: "#000000",
  selection: "#bcd8ff",
  string: "#008000",
  tag: "#808000",
  tooltipBackground: "#ffffff",
  type: "#660e7a",
};

const darkPalette: OxiDnsYamlPalette = {
  activeLine: "#263449",
  activeLineGutter: "#d8dee9",
  border: "#3b4252",
  boolean: "#cc7832",
  comment: "#808080",
  cursor: "#80c7ff",
  diffDeleteLine: "#3a2328",
  diffDeleteMarker: "#ff7b72",
  diffDeleteText: "#6b3036",
  diffInsertLine: "#193323",
  diffInsertMarker: "#7ee787",
  diffInsertText: "#24583a",
  foldHoverBackground: "#273449",
  foldHoverForeground: "#80c7ff",
  foldMarker: "#8b93a5",
  foreground: "#a9b7c6",
  gutterForeground: "#687083",
  key: "#9876aa",
  keyword: "#cc7832",
  nullValue: "#cc7832",
  number: "#6897bb",
  punctuation: "#cc7832",
  selection: "#214f7a",
  string: "#6a8759",
  tag: "#bbb529",
  tooltipBackground: "#202838",
  type: "#9876aa",
};

function paletteFor(theme: OxiDnsYamlTheme) {
  return theme === "light" ? lightPalette : darkPalette;
}

const darkHighlightStyle = HighlightStyle.define([
  { tag: tags.comment, color: darkPalette.comment, fontStyle: "italic" },
  { tag: tags.propertyName, color: darkPalette.key },
  { tag: tags.string, color: darkPalette.string },
  { tag: [tags.number, tags.integer, tags.float], color: darkPalette.number },
  { tag: tags.bool, color: darkPalette.boolean },
  { tag: tags.null, color: darkPalette.nullValue },
  { tag: tags.keyword, color: darkPalette.keyword },
  { tag: tags.punctuation, color: darkPalette.punctuation },
  { tag: tags.tagName, color: darkPalette.tag },
  { tag: tags.typeName, color: darkPalette.type },
]);

const lightHighlightStyle = HighlightStyle.define([
  { tag: tags.comment, color: lightPalette.comment, fontStyle: "italic" },
  { tag: tags.propertyName, color: lightPalette.key },
  { tag: tags.string, color: lightPalette.string },
  { tag: [tags.number, tags.integer, tags.float], color: lightPalette.number },
  { tag: tags.bool, color: lightPalette.boolean },
  { tag: tags.null, color: lightPalette.nullValue },
  { tag: tags.keyword, color: lightPalette.keyword },
  { tag: tags.punctuation, color: lightPalette.punctuation },
  { tag: tags.tagName, color: lightPalette.tag },
  { tag: tags.typeName, color: lightPalette.type },
]);

const scalarValuePattern =
  /^(\s*(?:-\s*)?[^#:\n][^:\n]*:\s*|\s*-\s+)([^#\s][^#]*?)(\s*(?:#.*)?)$/g;
const numericScalarPattern =
  /^[+-]?(?:(?:\d+\.?\d*)|(?:\.\d+))(?:e[+-]?\d+)?$/i;

function yamlScalarClass(rawValue: string) {
  const value = rawValue.trim();
  if (!value || /^["'|>[{&*!]/.test(value)) return null;
  if (numericScalarPattern.test(value)) return "oxidns-yaml-scalar-number";
  if (/^(?:true|false)$/i.test(value)) return "oxidns-yaml-scalar-boolean";
  if (/^(?:null|~)$/i.test(value)) return "oxidns-yaml-scalar-null";
  return "oxidns-yaml-scalar-string";
}

const yamlScalarMatcher = new MatchDecorator({
  regexp: scalarValuePattern,
  decorate(add, from, _to, match) {
    const rawValue = match[2] ?? "";
    const className = yamlScalarClass(rawValue);
    if (!className) return;
    const leadingWhitespace = rawValue.match(/^\s*/)?.[0].length ?? 0;
    const trailingWhitespace = rawValue.match(/\s*$/)?.[0].length ?? 0;
    const valueFrom = from + match[1].length + leadingWhitespace;
    const valueTo =
      from + match[1].length + rawValue.length - trailingWhitespace;
    if (valueFrom < valueTo) {
      add(valueFrom, valueTo, Decoration.mark({ class: className }));
    }
  },
});

const yamlScalarHighlighting = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = yamlScalarMatcher.createDeco(view);
    }

    update(update: ViewUpdate) {
      this.decorations = yamlScalarMatcher.updateDeco(update, this.decorations);
    }
  },
  {
    decorations: (plugin) => plugin.decorations,
  },
);

export function oxidnsYamlSyntaxHighlighting(theme: OxiDnsYamlTheme) {
  return [
    syntaxHighlighting(
      theme === "light" ? lightHighlightStyle : darkHighlightStyle,
    ),
    yamlScalarHighlighting,
  ];
}

export function oxidnsYamlCodeTheme(
  theme: OxiDnsYamlTheme,
  options: {
    contentPadding?: string;
    fillHeight?: boolean;
    fontSize?: string;
    lineHeight?: string;
    scrollerOverflow?: "auto" | "visible";
  } = {},
) {
  const palette = paletteFor(theme);
  const fillHeight = options.fillHeight ?? true;
  const lineHeight = options.lineHeight ?? "24px";

  return EditorView.theme(
    {
      "&": {
        backgroundColor: "transparent",
        color: palette.foreground,
        fontSize: options.fontSize ?? "14px",
        height: fillHeight ? "100%" : "auto",
        minHeight: "0",
      },
      ".cm-editor": {
        height: fillHeight ? "100%" : "auto",
        minHeight: "0",
      },
      ".cm-scroller": {
        fontFamily: editorFontFamily,
        height: fillHeight ? "100%" : "auto",
        lineHeight,
        overflow: options.scrollerOverflow ?? "auto",
      },
      ".cm-line": {
        lineHeight,
      },
      ".cm-content": {
        minHeight: fillHeight ? "100%" : "0",
        padding: options.contentPadding ?? "12px 0",
      },
      ".cm-gutters": {
        backgroundColor: "transparent",
        borderRightColor: palette.border,
        color: palette.gutterForeground,
      },
      ".cm-lineNumbers .cm-gutterElement": {
        minWidth: "36px",
        padding: "0 10px 0 4px",
        textAlign: "right",
      },
      ".cm-foldGutter": {
        width: "24px",
      },
      ".cm-foldGutter .cm-gutterElement": {
        alignItems: "center",
        boxSizing: "border-box",
        display: "flex",
        justifyContent: "center",
        minWidth: "24px",
        padding: "0",
      },
      ".cm-foldGutter span": {
        padding: "0",
      },
      ".oxidns-fold-marker": {
        alignItems: "center",
        borderRadius: "4px",
        color: palette.foldMarker,
        cursor: "pointer",
        display: "inline-flex",
        height: "18px",
        justifyContent: "center",
        width: "18px",
      },
      ".oxidns-fold-marker:hover": {
        backgroundColor: palette.foldHoverBackground,
        color: palette.foldHoverForeground,
      },
      ".oxidns-fold-marker svg": {
        height: "13px",
        transition: "transform 120ms ease",
        width: "13px",
      },
      ".oxidns-fold-marker[data-state='open'] svg": {
        transform: "rotate(90deg)",
      },
      ".cm-activeLineGutter": {
        backgroundColor: "transparent",
        color: palette.activeLineGutter,
      },
      ".cm-activeLine": {
        backgroundColor: palette.activeLine,
      },
      ".cm-cursor": {
        borderLeftColor: palette.cursor,
      },
      ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
        backgroundColor: palette.selection,
      },
      ".cm-tooltip": {
        backgroundColor: palette.tooltipBackground,
        borderColor: palette.border,
        color: palette.foreground,
      },
      ".cm-tooltip-autocomplete ul li[aria-selected]": {
        backgroundColor: palette.activeLine,
        color: palette.foreground,
      },
      ".oxidns-yaml-scalar-number": {
        color: palette.number,
      },
      ".oxidns-yaml-scalar-boolean": {
        color: palette.boolean,
      },
      ".oxidns-yaml-scalar-null": {
        color: palette.nullValue,
      },
      ".oxidns-yaml-scalar-string": {
        color: palette.string,
      },
      "&.cm-merge-a .cm-changedLine, .cm-deletedChunk": {
        backgroundColor: palette.diffDeleteLine,
      },
      "&.cm-merge-b .cm-changedLine, .cm-inlineChangedLine": {
        backgroundColor: palette.diffInsertLine,
      },
      "&.cm-merge-a .cm-changedText, .cm-deletedChunk .cm-deletedText": {
        backgroundColor: palette.diffDeleteText,
      },
      "&.cm-merge-b .cm-changedText": {
        backgroundColor: palette.diffInsertText,
      },
      "&.cm-merge-a .cm-changedLineGutter, .cm-deletedLineGutter": {
        backgroundColor: palette.diffDeleteMarker,
      },
      "&.cm-merge-b .cm-changedLineGutter": {
        backgroundColor: palette.diffInsertMarker,
      },
      ".cm-insertedLine, .cm-deletedLine, .cm-deletedLine del": {
        textDecoration: "none",
      },
      ".cm-collapsedLines": {
        backgroundColor: palette.activeLine,
        color: palette.gutterForeground,
        fontFamily: "inherit",
        fontSize: "12px",
      },
    },
    { dark: theme === "dark" },
  );
}

export function oxidnsYamlExtensions(
  context: OxiDnsYamlEditorContext,
  options: {
    backendDiagnostics?: OxiDnsYamlDiagnostic[];
    lineNumbers?: boolean;
    onSave?: () => void;
    readOnly?: boolean;
    theme?: OxiDnsYamlTheme;
  } = {},
): Extension[] {
  const theme = options.theme ?? "dark";
  return [
    yaml(),
    options.lineNumbers === false
      ? []
      : foldGutter({
          markerDOM: createFoldMarker,
        }),
    autocompletion({
      override: [
        (completionContext) =>
          buildCompletionResult(completionContext, context),
      ],
      activateOnTyping: true,
    }),
    hoverTooltip((view, pos) => buildHoverTooltip(view, pos, context)),
    linter(
      (view) =>
        buildDiagnostics(
          view.state,
          context,
          options.readOnly ? [] : (options.backendDiagnostics ?? []),
        ),
      { delay: 250 },
    ),
    history(),
    keymap.of([
      {
        key: "Mod-s",
        preventDefault: true,
        run() {
          options.onSave?.();
          return true;
        },
      },
      indentWithTab,
      ...historyKeymap,
    ]),
    EditorView.lineWrapping,
    EditorState.tabSize.of(2),
    oxidnsYamlCodeTheme(theme),
    oxidnsYamlSyntaxHighlighting(theme),
  ];
}

function createFoldMarker(open: boolean) {
  const marker = document.createElement("span");
  marker.className = "oxidns-fold-marker";
  marker.dataset.state = open ? "open" : "closed";
  marker.setAttribute("aria-hidden", "true");

  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "2.25");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");

  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", "m9 18 6-6-6-6");
  svg.append(path);
  marker.append(svg);

  return marker;
}

export function applyOxiDnsYamlDiagnostics(
  view: EditorView,
  context: OxiDnsYamlEditorContext,
  backendDiagnostics: OxiDnsYamlDiagnostic[] = [],
) {
  view.dispatch(
    setDiagnostics(view.state, [
      ...buildLocalDiagnostics(view.state, context),
      ...backendDiagnostics.flatMap((diagnostic) =>
        diagnosticFromBackend(view.state, diagnostic),
      ),
    ]),
  );
}

function buildCompletionResult(
  completionContext: CompletionContext,
  context: OxiDnsYamlEditorContext,
): CompletionResult | null {
  const line = completionContext.state.doc.lineAt(completionContext.pos);
  const prefix = completionContext.state.sliceDoc(
    line.from,
    completionContext.pos,
  );
  if (
    !completionContext.explicit &&
    !/[!$:\s"'-]$|[A-Za-z0-9_.-]$/.test(prefix)
  ) {
    return null;
  }

  const path = getYamlPath(completionContext.state, line.number);
  const { from, to } = getReplacementRange(completionContext.pos, prefix);
  const valueKey = getValueKey(prefix);
  const fields = fieldsForCompletion(
    completionContext.state,
    line.number,
    context,
    path,
  );
  const pluginKind =
    context.pluginKind ??
    inferConfigPluginKind(completionContext.state, line.number, path);
  const suggestions: Completion[] = [];

  if (isReferencePrefix(prefix)) {
    suggestions.push(
      ...pluginReferenceSuggestions(
        context,
        expectedReferenceTypes(context, fields, path, valueKey),
        prefix.trimEnd().endsWith("!$"),
      ),
    );
  }

  if (isKeyPosition(prefix)) {
    suggestions.push(...keySuggestions(context, path, fields));
  }

  if (context.variant === "config") {
    suggestions.push(
      ...configValueSuggestions(
        completionContext.state,
        context,
        path,
        valueKey,
      ),
    );
  }

  if (valueKey === "type" && isPluginKindValuePath(path)) {
    suggestions.push(...pluginKindSuggestions(context));
  }

  const field = findFieldForPath(fields, path, valueKey);
  if (field) {
    suggestions.push(
      ...fieldValueSuggestions(completionContext.state, context, field),
    );
  }

  if (shouldSuggestSequenceExpressions(context, pluginKind, path, valueKey)) {
    const types = expectedReferenceTypes(context, fields, path, valueKey);
    suggestions.push(...quickSetupSuggestions(context, types));
    suggestions.push(...pluginReferenceSuggestions(context, types));
    if (types?.includes("executor")) {
      suggestions.push(...controlSuggestions());
      const jumpCtx = detectJumpGotoExec(prefix);
      if (jumpCtx) {
        return {
          from: completionContext.pos - jumpCtx.prefixLength,
          to,
          options: dedupeSuggestions([
            ...suggestions,
            ...jumpGotoTagSuggestions(context, jumpCtx.keyword),
          ]),
          validFor: /^[A-Za-z0-9_.!$ -]*$/,
        };
      }
    }
  }

  const options = dedupeSuggestions(suggestions);
  return options.length
    ? {
        from,
        to,
        options,
        validFor: /^[A-Za-z0-9_.!$-]*$/,
      }
    : null;
}

function buildHoverTooltip(
  view: EditorView,
  pos: number,
  context: OxiDnsYamlEditorContext,
): Tooltip | null {
  const token = getTokenAtPosition(view.state, pos);
  if (!token) return null;
  const locale = contextLocale(context);
  const clean = token.text.replace(/^!?\$/, "");
  const plugin = context.plugins?.find((entry) => entry.name === clean);

  if (plugin) {
    return {
      pos: token.from,
      end: token.to,
      above: true,
      create() {
        return {
          dom: tooltipDom(
            plugin.name,
            `${pluginTypeLabel(plugin.type, locale)} / ${plugin.pluginKind}`,
          ),
        };
      },
    };
  }

  const definition = localizedPluginKindDefinition(clean, locale);
  if (definition) {
    return {
      pos: token.from,
      end: token.to,
      above: true,
      create() {
        return {
          dom: tooltipDom(
            definition.kind,
            `${pluginTypeLabel(definition.type, locale)} · ${definition.description}`,
          ),
        };
      },
    };
  }

  return null;
}

function tooltipDom(title: string, detail: string) {
  const wrapper = document.createElement("div");
  wrapper.className = "space-y-1 p-2 text-xs";
  const heading = document.createElement("div");
  heading.className = "font-semibold";
  heading.textContent = title;
  const body = document.createElement("div");
  body.className = "text-muted-foreground";
  body.textContent = detail;
  wrapper.append(heading, body);
  return wrapper;
}

function buildDiagnostics(
  state: EditorState,
  context: OxiDnsYamlEditorContext,
  backendDiagnostics: OxiDnsYamlDiagnostic[] = [],
) {
  return [
    ...buildLocalDiagnostics(state, context),
    ...backendDiagnostics.flatMap((diagnostic) =>
      diagnosticFromBackend(state, diagnostic),
    ),
  ];
}

function buildLocalDiagnostics(
  state: EditorState,
  context: OxiDnsYamlEditorContext,
): Diagnostic[] {
  const locale = contextLocale(context);
  const diagnostics: Diagnostic[] = [];
  const pluginTags = new Set(
    (context.plugins ?? []).map((plugin) => plugin.name),
  );
  const knownPluginKinds = new Set(
    pluginKindDefinitions.map((definition) => definition.kind),
  );

  for (let lineNumber = 1; lineNumber <= state.doc.lines; lineNumber += 1) {
    const line = state.doc.line(lineNumber);
    const commentStart = line.text.indexOf("#");
    const checkText =
      commentStart >= 0 ? line.text.slice(0, commentStart) : line.text;

    for (const match of checkText.matchAll(/!?\$([A-Za-z0-9_.-]+)/g)) {
      const tag = match[1];
      if (!tag || pluginTags.has(tag)) continue;
      const start = line.from + (match.index ?? 0);
      diagnostics.push({
        severity: "warning",
        message: translate(locale, WEBUI.plugins.missingReference, { tag }),
        from: start,
        to: start + match[0].length,
        source: "OxiDNS",
      });
    }

    if (
      context.variant === "config" &&
      getYamlPath(state, lineNumber).includes("plugins")
    ) {
      const typeMatch = checkText.match(
        /^(\s*)(?:-\s*)?type\s*:\s*["']?([A-Za-z0-9_-]+)/,
      );
      const pluginKind = typeMatch?.[2];
      if (pluginKind && !knownPluginKinds.has(pluginKind)) {
        const start = line.from + (typeMatch[0].lastIndexOf(pluginKind) ?? 0);
        diagnostics.push({
          severity: "warning",
          message: translate(locale, WEBUI.plugins.missingPluginType, {
            kind: pluginKind,
          }),
          from: start,
          to: start + pluginKind.length,
          source: "OxiDNS",
        });
      }
    }

    if (context.variant === "config") {
      const path = getYamlPath(state, lineNumber);
      const tagMatch = checkText.match(
        /^(\s*)(?:-\s*)?tag\s*:\s*(?:"([^"]*)"|'([^']*)'|([^\s#]+))/,
      );
      const tag = tagMatch?.[2] ?? tagMatch?.[3] ?? tagMatch?.[4];
      if (
        tag &&
        path[0] === "plugins" &&
        path[path.length - 1] === "tag" &&
        !path.includes("args")
      ) {
        const validationError = validatePluginTag(tag);
        if (validationError) {
          const start = line.from + (tagMatch?.[0].lastIndexOf(tag) ?? 0);
          diagnostics.push({
            severity: "error",
            message: translate(
              locale,
              pluginTagValidationMessageKey(validationError),
            ),
            from: start,
            to: start + tag.length,
            source: "OxiDNS",
          });
        }
      }
    }

    const jumpGotoMatch = checkText.match(/\b(jump|goto)\s+([A-Za-z0-9_.-]+)/);
    if (jumpGotoMatch) {
      const tag = jumpGotoMatch[2];
      if (!pluginTags.has(tag)) {
        const tagStart =
          (jumpGotoMatch.index ?? 0) +
          jumpGotoMatch[0].length -
          jumpGotoMatch[2].length;
        const start = line.from + tagStart;
        diagnostics.push({
          severity: "warning",
          message: translate(locale, WEBUI.plugins.missingReference, { tag }),
          from: start,
          to: start + tag.length,
          source: "OxiDNS",
        });
      }
    }
  }

  return diagnostics;
}

function diagnosticFromBackend(
  state: EditorState,
  diagnostic: OxiDnsYamlDiagnostic,
): Diagnostic[] {
  const message = diagnostic.message;
  const lineNumber =
    diagnostic.line && diagnostic.column
      ? Math.min(diagnostic.line, state.doc.lines)
      : 1;
  const line = state.doc.line(lineNumber);
  const from = Math.min(
    line.to,
    line.from + Math.max(0, (diagnostic.column ?? 1) - 1),
  );
  const to = Math.max(
    Math.min(line.to, line.from + Math.max(0, (diagnostic.end_column ?? 2) - 1)),
    Math.min(line.to, from + 1),
  );
  return [
    {
      severity: diagnosticSeverity(diagnostic.severity),
      message,
      from,
      to,
      source: "OxiDNS",
    },
  ];
}

function diagnosticSeverity(
  severity: OxiDnsYamlDiagnostic["severity"],
): Diagnostic["severity"] {
  if (severity === "warning") return "warning";
  if (severity === "info") return "info";
  return "error";
}

function contextLocale(context: OxiDnsYamlEditorContext): Locale {
  return context.locale ?? DEFAULT_LOCALE;
}

function localizedPluginKindDefinition(kind: string, locale: Locale) {
  return (
    getLocalizedPluginKindDefinition(kind, locale) ??
    getPluginKindDefinition(kind)
  );
}

function keySuggestions(
  context: OxiDnsYamlEditorContext,
  path: string[],
  fields: ConfigField[] | undefined,
): Completion[] {
  if (context.variant === "config") {
    const subKeys = getOxiDnsConfigSubKeys(path);
    if (subKeys !== null) {
      return subKeys.map((key) => keyCompletion(key));
    }
  }

  return (fields ?? []).map((field) => keyCompletion(field.key));
}

function pluginKindSuggestions(context: OxiDnsYamlEditorContext): Completion[] {
  const locale = contextLocale(context);
  return getLocalizedPluginKindDefinitions(locale).map((definition) => ({
    label: definition.kind,
    type: "class",
    apply: definition.kind,
    detail: pluginTypeLabel(definition.type, locale),
    info: definition.description,
    sortText: `0-${definition.type}-${definition.kind}`,
  }));
}

function logLevelSuggestions(context: OxiDnsYamlEditorContext): Completion[] {
  const locale = contextLocale(context);
  return OXIDNS_LOG_LEVELS.map((level) => ({
    label: level,
    type: "enum",
    apply: level,
    detail: translate(locale, WEBUI.common.logLevel),
    sortText: `0-${level}`,
  }));
}

function configValueSuggestions(
  state: EditorState,
  context: OxiDnsYamlEditorContext,
  path: string[],
  valueKey: string | null,
): Completion[] {
  const suggestions =
    valueKey === "level" && path.includes("log")
      ? logLevelSuggestions(context)
      : getOxiDnsConfigValueSuggestions(path, valueKey).map(
          configValueCompletion,
        );

  if (shouldSuggestOutboundProfiles(path, valueKey)) {
    suggestions.push(...outboundProfileSuggestions(state, context));
  }

  return suggestions;
}

function configValueCompletion(
  suggestion: OxiDnsConfigValueSuggestion,
): Completion {
  return {
    label: suggestion.label,
    type: suggestion.type ?? "text",
    apply: suggestion.apply ?? suggestion.label,
    detail: suggestion.detail,
    sortText: `0-${suggestion.label}`,
  };
}

function shouldSuggestOutboundProfiles(
  path: string[],
  valueKey: string | null,
) {
  if (valueKey === "outbound") return true;
  return (
    valueKey === "default" && path[0] === "network" && path[1] === "outbound"
  );
}

function isPluginKindValuePath(path: string[]) {
  if (path[0] !== "plugins") return false;
  return !path.includes("args");
}

function outboundProfileSuggestions(
  state: EditorState,
  context: OxiDnsYamlEditorContext,
): Completion[] {
  return outboundProfileNames(state, context).map((profile) => ({
    label: profile,
    type: "variable",
    apply: profile,
    sortText: `0-${profile}`,
  }));
}

function outboundProfileNames(
  state: EditorState,
  context: OxiDnsYamlEditorContext,
): string[] {
  if (context.outboundProfileNames) return context.outboundProfileNames;
  if (context.variant !== "config") return [];
  try {
    const document = parseDocument(state.doc.toString());
    if (document.errors.length > 0) return [];
    return extractOutboundProfileNames(document.toJSON());
  } catch {
    return [];
  }
}

function pluginReferenceSuggestions(
  context: OxiDnsYamlEditorContext,
  referenceTypes?: PluginType[],
  inverted = false,
): Completion[] {
  const locale = contextLocale(context);
  const prefix = inverted ? "!$" : "$";
  return (context.plugins ?? [])
    .filter((plugin) => plugin.name !== context.currentPluginName)
    .filter(
      (plugin) =>
        !referenceTypes ||
        referenceTypes.length === 0 ||
        referenceTypes.includes(plugin.type),
    )
    .map((plugin) => ({
      label: `${prefix}${plugin.name}`,
      type: "variable",
      apply: `${prefix}${plugin.name}`,
      detail: `${pluginTypeLabel(plugin.type, locale)} / ${plugin.pluginKind}`,
      info: localizedPluginKindDefinition(plugin.pluginKind, locale)
        ?.description,
      sortText: `1-${plugin.type}-${plugin.name}`,
    }));
}

function quickSetupSuggestions(
  context: OxiDnsYamlEditorContext,
  types?: PluginType[],
): Completion[] {
  const locale = contextLocale(context);
  return getLocalizedPluginKindDefinitions(locale)
    .filter((definition) => definition.quickSetup)
    .filter(
      (definition) =>
        !types || types.length === 0 || types.includes(definition.type),
    )
    .map((definition) => ({
      label: definition.kind,
      type: "function",
      apply: definition.quickSetup?.paramPlaceholder
        ? `${definition.kind} `
        : definition.kind,
      detail: `${translate(locale, WEBUI.plugins.quickSetup)} · ${pluginTypeLabel(
        definition.type,
        locale,
      )}`,
      info: definition.quickSetup?.paramPlaceholder ?? definition.description,
      sortText: `2-${definition.type}-${definition.kind}`,
    }));
}

function controlSuggestions(): Completion[] {
  const controls = sequenceControls.map((control) => ({
    label: control,
    type: "keyword",
    apply: control,
    sortText: `3-${control}`,
  }));
  const examples = sequenceControlExamples.map((control) => ({
    label: control,
    type: "text",
    apply: control,
    sortText: `3-${control}`,
  }));
  return [...controls, ...examples];
}

function detectJumpGotoExec(
  prefix: string,
): { keyword: "jump" | "goto"; prefixLength: number } | null {
  const match = prefix.match(/\b(jump|goto)\s+([A-Za-z0-9_.-]*)$/);
  if (!match) return null;
  return {
    keyword: match[1] as "jump" | "goto",
    prefixLength: match[0].length,
  };
}

function jumpGotoTagSuggestions(
  context: OxiDnsYamlEditorContext,
  keyword: "jump" | "goto",
): Completion[] {
  const locale = contextLocale(context);
  return (context.plugins ?? [])
    .filter((plugin) => plugin.name !== context.currentPluginName)
    .filter((plugin) => plugin.type === "executor")
    .map((plugin) => ({
      label: `${keyword} ${plugin.name}`,
      type: "variable",
      apply: `${keyword} ${plugin.name}`,
      detail: `${pluginTypeLabel(plugin.type, locale)} / ${plugin.pluginKind}`,
      info: localizedPluginKindDefinition(plugin.pluginKind, locale)
        ?.description,
      sortText: `0-${keyword}-${plugin.name}`,
    }));
}

function fieldValueSuggestions(
  state: EditorState,
  context: OxiDnsYamlEditorContext,
  field: ConfigField,
): Completion[] {
  const locale = contextLocale(context);
  if (field.dynamicOptions === "outboundProfiles") {
    return outboundProfileSuggestions(state, context);
  }

  if (field.type === "select") {
    return (
      field.options?.map((option) => ({
        label: String(option.value),
        type: "enum",
        apply: String(option.value),
        detail: option.label,
      })) ?? []
    );
  }

  if (field.type !== "reference") return [];

  const prefix = field.referencePrefix ?? "";
  return (context.plugins ?? [])
    .filter(
      (plugin) =>
        !field.referenceTypes ||
        field.referenceTypes.length === 0 ||
        field.referenceTypes.includes(plugin.type),
    )
    .filter(
      (plugin) =>
        !field.referencePlugins ||
        field.referencePlugins.length === 0 ||
        field.referencePlugins.includes(plugin.pluginKind),
    )
    .flatMap((plugin) => {
      const base = {
        type: "variable",
        detail: `${pluginTypeLabel(plugin.type, locale)} / ${plugin.pluginKind}`,
      };
      const items: Completion[] = [
        {
          ...base,
          label: `${prefix}${plugin.name}`,
          apply: `${prefix}${plugin.name}`,
          sortText: `1-${plugin.type}-${plugin.name}`,
        },
      ];
      if (field.allowInvert || field.referenceTypes?.includes("matcher")) {
        items.push({
          ...base,
          label: `!${prefix}${plugin.name}`,
          apply: `!${prefix}${plugin.name}`,
          sortText: `1-invert-${plugin.type}-${plugin.name}`,
        });
      }
      return items;
    });
}

function keyCompletion(key: string): Completion {
  return {
    label: key,
    type: "property",
    apply: `${key}: `,
    sortText: `0-${key}`,
  };
}

function expectedReferenceTypes(
  context: OxiDnsYamlEditorContext,
  fields: ConfigField[] | undefined,
  path: string[],
  valueKey: string | null,
): PluginType[] | undefined {
  const field = findFieldForPath(fields, path, valueKey);
  if (field?.referenceTypes?.length) return field.referenceTypes;

  const joined = path.join(".");
  if (valueKey === "matches" || joined.includes("matches")) return ["matcher"];
  if (
    valueKey === "exec" ||
    valueKey === "entry" ||
    valueKey === "primary" ||
    valueKey === "secondary" ||
    valueKey === "executors" ||
    joined.includes("executors")
  ) {
    return ["executor"];
  }
  if (valueKey?.includes("provider") || joined.includes("provider")) {
    return ["provider"];
  }
  if (context.variant === "sequence") return ["matcher", "executor"];
  return undefined;
}

function shouldSuggestSequenceExpressions(
  context: OxiDnsYamlEditorContext,
  pluginKind: string | undefined,
  path: string[],
  valueKey: string | null,
) {
  if (context.variant === "sequence") return true;
  if (pluginKind === "sequence" || pluginKind === "cron") {
    const joined = path.join(".");
    return (
      valueKey === "matches" ||
      valueKey === "exec" ||
      valueKey === "executors" ||
      joined.includes("matches") ||
      joined.includes("executors")
    );
  }
  return false;
}

function fieldsForCompletion(
  state: EditorState,
  lineNumber: number,
  context: OxiDnsYamlEditorContext,
  path: string[],
): ConfigField[] | undefined {
  if (context.variant === "plugin-args") {
    return fieldsForPath(context.fields, path);
  }
  if (context.variant === "config" && isConfigPluginArgsPath(path)) {
    const pluginKind = inferConfigPluginKind(state, lineNumber, path);
    return fieldsForPath(
      getPluginKindDefinition(pluginKind ?? "")?.configSchema,
      path,
    );
  }
  return context.fields;
}

function fieldsForPath(
  fields: ConfigField[] | undefined,
  path: string[],
): ConfigField[] | undefined {
  if (!fields || path.length === 0) return fields;
  let normalizedPath = path.filter(
    (part) => part !== "plugins" && part !== "args",
  );
  let current: ConfigField[] | undefined = fields;
  const argsField =
    fields.length === 1 && fields[0].key === "args" ? fields[0] : undefined;
  const argsChildFields = childFields(argsField);
  if (argsChildFields) {
    current = argsChildFields;
    normalizedPath = path.filter((part) => part !== "plugins");
    if (normalizedPath[0] === "args") {
      normalizedPath = normalizedPath.slice(1);
    }
  }

  for (const key of normalizedPath.slice(0, -1)) {
    const field = current?.find((entry) => entry.key === key);
    current = childFields(field);
    if (!current) break;
  }

  return current ?? fields;
}

function isConfigPluginArgsPath(path: string[]) {
  return path[0] === "plugins" && path.includes("args");
}

function childFields(
  field: ConfigField | undefined,
): ConfigField[] | undefined {
  if (!field) return undefined;
  if (field.type === "object") return field.fields;
  if (field.type === "array") {
    if (field.item?.type === "object") return field.item.fields;
    const objectOption = field.itemOptions?.find(
      (item): item is Extract<ConfigFieldChild, { type: "object" }> =>
        item.type === "object",
    );
    return objectOption?.fields;
  }
  return undefined;
}

function findFieldForKey(
  fields: ConfigField[] | undefined,
  key: string | null,
): ConfigField | undefined {
  if (!fields || !key) return undefined;
  for (const field of fields) {
    if (field.key === key) return field;
    const nested = findFieldForKey(childFields(field), key);
    if (nested) return nested;
  }
  return undefined;
}

function findFieldForPath(
  fields: ConfigField[] | undefined,
  path: string[],
  key: string | null,
): ConfigField | undefined {
  if (!fields || !key) return undefined;
  const direct = fieldsForPath(fields, path)?.find(
    (field) => field.key === key,
  );
  return direct ?? findFieldForKey(fields, key);
}

function inferConfigPluginKind(
  state: EditorState,
  lineNumber: number,
  path: string[],
): string | undefined {
  if (path[0] !== "plugins") return undefined;
  const pluginsLine = findPluginsLine(state, lineNumber);
  if (!pluginsLine) return undefined;

  const pluginItemIndent = findPluginItemIndent(
    state,
    pluginsLine.lineNumber,
    lineNumber,
    pluginsLine.indent,
  );
  if (pluginItemIndent === undefined) return undefined;

  const itemStart = findCurrentPluginItemStart(
    state,
    pluginsLine.lineNumber,
    lineNumber,
    pluginItemIndent,
  );
  if (itemStart === undefined) return undefined;

  return findPluginKindInItem(state, itemStart, lineNumber, pluginItemIndent);
}

function findPluginsLine(state: EditorState, lineNumber: number) {
  for (let index = lineNumber; index >= 1; index -= 1) {
    const line = state.doc.line(index).text;
    const topLevel = line.match(/^(\s*)plugins\s*:/);
    if (topLevel) return { lineNumber: index, indent: topLevel[1].length };
    if (
      index !== lineNumber &&
      /^\S/.test(line) &&
      !/^plugins\s*:/.test(line)
    ) {
      return null;
    }
  }
  return null;
}

function findPluginItemIndent(
  state: EditorState,
  pluginsLineNumber: number,
  lineNumber: number,
  pluginsIndent: number,
) {
  let itemIndent: number | undefined;
  for (let index = pluginsLineNumber + 1; index <= lineNumber; index += 1) {
    const line = state.doc.line(index).text;
    const match = line.match(/^(\s*)-\s+/);
    if (!match) continue;
    const indent = match[1].length;
    if (indent <= pluginsIndent) continue;
    itemIndent =
      itemIndent === undefined ? indent : Math.min(itemIndent, indent);
  }
  return itemIndent;
}

function findCurrentPluginItemStart(
  state: EditorState,
  pluginsLineNumber: number,
  lineNumber: number,
  pluginItemIndent: number,
) {
  for (let index = lineNumber; index > pluginsLineNumber; index -= 1) {
    const line = state.doc.line(index).text;
    const match = line.match(/^(\s*)-\s+/);
    if (match && match[1].length === pluginItemIndent) return index;
  }
  return undefined;
}

function findPluginKindInItem(
  state: EditorState,
  itemStart: number,
  lineNumber: number,
  pluginItemIndent: number,
) {
  for (let index = itemStart; index <= lineNumber; index += 1) {
    const line = state.doc.line(index).text;
    if (index > itemStart) {
      const nextItem = line.match(/^(\s*)-\s+/);
      if (nextItem && nextItem[1].length === pluginItemIndent) break;
    }
    const type = line.match(/^\s*(?:-\s*)?type\s*:\s*["']?([A-Za-z0-9_-]+)/);
    if (type?.[1]) return type[1];
  }
  return undefined;
}

function getYamlPath(state: EditorState, lineNumber: number) {
  const stack: Array<{ indent: number; key: string }> = [];
  for (let index = 1; index <= lineNumber; index += 1) {
    const raw = state.doc.line(index).text;
    const currentIndent = raw.match(/^(\s*)/)?.[1].length ?? 0;
    const match = raw.match(/^(\s*)(?:-\s*)?([A-Za-z0-9_-]+)\s*:/);
    if (!match) {
      if (index === lineNumber) {
        while (
          stack.length &&
          stack[stack.length - 1].indent >= currentIndent
        ) {
          stack.pop();
        }
      }
      continue;
    }
    const indent = match[1].length;
    const key = match[2];
    while (stack.length && stack[stack.length - 1].indent >= indent) {
      stack.pop();
    }
    stack.push({ indent, key });
  }
  return stack.map((item) => item.key);
}

function getReplacementRange(pos: number, prefix: string) {
  const match = prefix.match(/!?[$]?[A-Za-z0-9_.-]*$/);
  const token = match?.[0] ?? "";
  return {
    from: pos - token.length,
    to: pos,
  };
}

function getValueKey(prefix: string) {
  const match = prefix.match(/(?:^|\s)([A-Za-z0-9_-]+)\s*:\s*[^:]*$/);
  return match?.[1] ?? null;
}

function isKeyPosition(prefix: string) {
  const trimmed = prefix.trimStart();
  return (
    !trimmed ||
    /^-\s*[A-Za-z0-9_-]*$/.test(trimmed) ||
    /^[A-Za-z0-9_-]*$/.test(trimmed)
  );
}

function isReferencePrefix(prefix: string) {
  return /(?:^|\s)!?\$[A-Za-z0-9_.-]*$/.test(prefix);
}

function getTokenAtPosition(state: EditorState, pos: number) {
  const line = state.doc.lineAt(pos);
  const offset = pos - line.from;
  const left = line.text.slice(0, offset);
  const right = line.text.slice(offset);
  const leftMatch = left.match(/!?[$]?[A-Za-z0-9_.-]*$/);
  const rightMatch = right.match(/^[A-Za-z0-9_.-]*/);
  const text = `${leftMatch?.[0] ?? ""}${rightMatch?.[0] ?? ""}`;
  if (!text) return null;
  const from = pos - (leftMatch?.[0].length ?? text.length);
  return {
    text,
    from,
    to: from + text.length,
  };
}

function dedupeSuggestions(items: Completion[]) {
  const seen = new Set<string>();
  return items.filter((item) => {
    const key = item.label;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}
