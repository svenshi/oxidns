/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { EditorState, StateEffect } from "@codemirror/state";
import {
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  lineNumbers as codeMirrorLineNumbers,
} from "@codemirror/view";
import { useTheme } from "next-themes";
import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { ConfigField } from "@/lib/plugin-definitions";
import { ConfigValidationError, validateConfigText } from "@/lib/oxidns-api";
import {
  applyOxiDnsYamlDiagnostics,
  oxidnsYamlExtensions,
  type OxiDnsYamlDiagnostic,
  type OxiDnsYamlEditorVariant,
} from "@/lib/oxidns-yaml-codemirror";
import type { PluginInstance } from "@/lib/types";
import { cn } from "@/lib/utils";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

export interface YamlEditorHandle {
  jumpToLine: (line: number) => void;
}

interface YamlEditorProps {
  value: string;
  onChange?: (value: string) => void;
  readOnly?: boolean;
  className?: string;
  lineNumbers?: boolean;
  variant?: OxiDnsYamlEditorVariant;
  plugins?: PluginInstance[];
  pluginKind?: string;
  fields?: ConfigField[];
  currentPluginName?: string;
  outboundProfileNames?: string[];
  /** Bound to Cmd+S (macOS) / Ctrl+S (Windows/Linux). */
  onSave?: () => void;
  /** Run the backend /config/validate pass. Disable in offline mode. */
  backendValidation?: boolean;
}

export const YamlEditor = forwardRef<YamlEditorHandle, YamlEditorProps>(
  function YamlEditor(
    {
      value,
      onChange,
      readOnly = false,
      className,
      lineNumbers = true,
      variant = "generic",
      plugins,
      pluginKind,
      fields,
      currentPluginName,
      outboundProfileNames,
      onSave,
      backendValidation = true,
    },
    ref,
  ) {
    const { locale, t } = useI18n();
    const { resolvedTheme } = useTheme();
    const containerRef = useRef<HTMLDivElement | null>(null);
    const viewRef = useRef<EditorView | null>(null);
    const validationSeqRef = useRef(0);
    const onChangeRef = useRef(onChange);
    const onSaveRef = useRef(onSave);
    const externalUpdateRef = useRef(false);
    onChangeRef.current = onChange;
    onSaveRef.current = onSave;

    const [backendDiagnostics, setBackendDiagnostics] = useState<
      OxiDnsYamlDiagnostic[]
    >([]);
    const context = useMemo(
      () => ({
        variant,
        locale,
        plugins,
        pluginKind,
        fields,
        currentPluginName,
        outboundProfileNames,
      }),
      [
        variant,
        locale,
        plugins,
        pluginKind,
        fields,
        currentPluginName,
        outboundProfileNames,
      ],
    );
    const editorTheme = resolvedTheme === "light" ? "light" : "dark";
    const handleSave = useCallback(() => {
      onSaveRef.current?.();
    }, []);

    const extensions = useMemo(
      () => [
        lineNumbers ? codeMirrorLineNumbers() : [],
        highlightActiveLine(),
        lineNumbers ? highlightActiveLineGutter() : [],
        EditorState.readOnly.of(readOnly),
        EditorView.editable.of(!readOnly),
        EditorView.updateListener.of((update) => {
          if (!update.docChanged || externalUpdateRef.current) return;
          onChangeRef.current?.(update.state.doc.toString());
        }),
        ...oxidnsYamlExtensions(context, {
          backendDiagnostics:
            variant === "config" && !readOnly && backendValidation
              ? backendDiagnostics
              : [],
          lineNumbers,
          onSave: handleSave,
          readOnly,
          theme: editorTheme,
        }),
      ],
      [
        backendDiagnostics,
        backendValidation,
        context,
        editorTheme,
        handleSave,
        lineNumbers,
        readOnly,
        variant,
      ],
    );

    useLayoutEffect(() => {
      const container = containerRef.current;
      if (!container || viewRef.current) return;
      const view = new EditorView({
        parent: container,
        state: EditorState.create({
          doc: value,
          extensions,
        }),
      });
      viewRef.current = view;

      return () => {
        view.destroy();
        viewRef.current = null;
      };
      // The initial view is created once; extension and value updates are
      // handled by the effects below so scroll position is not reset.
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    useEffect(() => {
      const view = viewRef.current;
      if (!view) return;
      view.dispatch({ effects: StateEffect.reconfigure.of(extensions) });
    }, [extensions]);

    useEffect(() => {
      const view = viewRef.current;
      if (!view) return;
      const currentValue = view.state.doc.toString();
      if (currentValue === value) return;
      externalUpdateRef.current = true;
      view.dispatch({
        changes: { from: 0, to: currentValue.length, insert: value },
      });
      externalUpdateRef.current = false;
    }, [value]);

    useImperativeHandle(
      ref,
      () => ({
        jumpToLine(lineNumber: number) {
          const view = viewRef.current;
          if (!view) return;
          const line = view.state.doc.line(
            Math.min(Math.max(1, lineNumber), view.state.doc.lines),
          );
          view.dispatch({
            selection: { anchor: line.from },
            effects: EditorView.scrollIntoView(line.from, { y: "center" }),
          });
          view.focus();
        },
      }),
      [],
    );

    useEffect(() => {
      if (variant !== "config" || readOnly || !backendValidation) {
        return;
      }

      const seq = validationSeqRef.current + 1;
      validationSeqRef.current = seq;
      const timer = window.setTimeout(() => {
        void validateConfigText(value)
          .then(() => {
            if (validationSeqRef.current === seq) setBackendDiagnostics([]);
          })
          .catch((error: unknown) => {
            if (validationSeqRef.current !== seq) return;
            if (error instanceof ConfigValidationError) {
              setBackendDiagnostics(
                error.diagnosticDetails.length > 0
                  ? error.diagnosticDetails
                  : error.diagnostics.map((message) => ({ message })),
              );
              return;
            }
            setBackendDiagnostics([
              {
                message:
                  error instanceof Error
                    ? error.message
                    : t(WEBUI.configEditor.configValidationFailed),
              },
            ]);
          });
      }, 800);

      return () => window.clearTimeout(timer);
    }, [backendValidation, readOnly, t, value, variant]);

    useEffect(() => {
      const view = viewRef.current;
      if (!view) return;
      applyOxiDnsYamlDiagnostics(
        view,
        context,
        variant === "config" && !readOnly && backendValidation
          ? backendDiagnostics
          : [],
      );
    }, [
      backendDiagnostics,
      backendValidation,
      context,
      readOnly,
      value,
      variant,
    ]);

    return (
      <div
        ref={containerRef}
        className={cn(
          "relative h-full min-h-0 overflow-hidden rounded-md border bg-muted/30 font-mono text-sm [&_.cm-editor]:h-full [&_.cm-editor]:min-h-0 [&_.cm-scroller]:h-full [&_.cm-scroller]:overflow-auto",
          className,
        )}
      />
    );
  },
);
