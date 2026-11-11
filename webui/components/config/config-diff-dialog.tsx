/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { MergeView } from "@codemirror/merge";
import { yaml } from "@codemirror/lang-yaml";
import { EditorState, type Extension } from "@codemirror/state";
import {
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  lineNumbers,
} from "@codemirror/view";
import { useTheme } from "next-themes";
import { useEffect, useMemo, useRef } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import {
  oxidnsYamlCodeTheme,
  oxidnsYamlSyntaxHighlighting,
} from "@/lib/oxidns-yaml-codemirror";

interface ConfigDiffDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Left side — the selected historical snapshot. */
  original: string;
  /** Right side — the current editor content. */
  modified: string;
  originalTitle?: string;
  modifiedTitle?: string;
}

const MIN_DIFF_VIEW_HEIGHT = 160;

interface CodeMirrorMergeViewProps {
  original: string;
  modified: string;
  extensions: Extension[];
}

function CodeMirrorMergeView({
  original,
  modified,
  extensions,
}: CodeMirrorMergeViewProps) {
  const mergeRootRef = useRef<HTMLDivElement | null>(null);
  const mergeViewRef = useRef<MergeView | null>(null);

  useEffect(() => {
    const mergeRoot = mergeRootRef.current;
    if (!mergeRoot) return;

    let cancelled = false;
    let animationFrame: number | null = null;

    const destroyMergeView = () => {
      mergeViewRef.current?.destroy();
      mergeViewRef.current = null;
      mergeRoot.replaceChildren();
    };

    const measureEditors = () => {
      const view = mergeViewRef.current;
      if (!view) return;
      view.a.requestMeasure();
      view.b.requestMeasure();
    };

    const hasRenderableSize = () => {
      const { height } = mergeRoot.getBoundingClientRect();
      return height >= MIN_DIFF_VIEW_HEIGHT;
    };

    const cancelScheduledMount = () => {
      if (animationFrame === null) return;
      window.cancelAnimationFrame(animationFrame);
      animationFrame = null;
    };

    const mountMergeView = () => {
      animationFrame = null;
      if (cancelled) return;
      if (!hasRenderableSize()) return;

      destroyMergeView();
      mergeViewRef.current = new MergeView({
        a: {
          doc: original,
          extensions,
        },
        b: {
          doc: modified,
          extensions,
        },
        parent: mergeRoot,
        orientation: "a-b",
        revertControls: undefined,
        highlightChanges: true,
        gutter: true,
        collapseUnchanged: {
          margin: 3,
          minSize: 8,
        },
        diffConfig: {
          scanLimit: 10_000,
          timeout: 1_000,
        },
      });
      measureEditors();
    };

    const scheduleMount = () => {
      cancelScheduledMount();
      animationFrame = window.requestAnimationFrame(mountMergeView);
    };

    const resizeObserver = new ResizeObserver(() => {
      if (mergeViewRef.current) {
        measureEditors();
        return;
      }
      scheduleMount();
    });

    resizeObserver.observe(mergeRoot);
    scheduleMount();

    return () => {
      cancelled = true;
      cancelScheduledMount();
      resizeObserver.disconnect();
      destroyMergeView();
    };
  }, [extensions, modified, original]);

  return <div ref={mergeRootRef} className="h-full min-h-0 overflow-hidden" />;
}

export function ConfigDiffDialog({
  open,
  onOpenChange,
  original,
  modified,
  originalTitle,
  modifiedTitle,
}: ConfigDiffDialogProps) {
  const { t } = useI18n();
  const { resolvedTheme } = useTheme();
  const editorTheme = resolvedTheme === "light" ? "light" : "dark";
  const displayOriginalTitle =
    originalTitle ?? t(WEBUI.configEditor.historySnapshot);
  const displayModifiedTitle =
    modifiedTitle ?? t(WEBUI.configEditor.currentEditor);

  const extensions = useMemo<Extension[]>(
    () => [
      lineNumbers(),
      highlightActiveLine(),
      highlightActiveLineGutter(),
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      yaml(),
      oxidnsYamlCodeTheme(editorTheme, {
        contentPadding: "8px 0",
        fillHeight: false,
        fontSize: "13px",
        lineHeight: "22px",
        scrollerOverflow: "visible",
      }),
      oxidnsYamlSyntaxHighlighting(editorTheme),
    ],
    [editorTheme],
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="h-[min(82vh,760px)] max-w-[min(1180px,calc(100vw-3rem))] grid-rows-[auto_auto_minmax(0,1fr)] gap-3 sm:max-w-[min(1180px,calc(100vw-3rem))]">
        <DialogHeader>
          <DialogTitle>{t(WEBUI.configEditor.diffTitle)}</DialogTitle>
          <DialogDescription>
            {t(WEBUI.configEditor.diffDescription, {
              original: displayOriginalTitle,
              modified: displayModifiedTitle,
            })}
          </DialogDescription>
        </DialogHeader>
        <div className="grid grid-cols-2 gap-2 text-xs text-muted-foreground">
          <div className="truncate font-medium">{displayOriginalTitle}</div>
          <div className="truncate font-medium">{displayModifiedTitle}</div>
        </div>
        <div className="oxidns-config-diff-host relative h-full min-h-0 overflow-hidden rounded-md border bg-muted/20 font-mono text-sm">
          <CodeMirrorMergeView
            original={original}
            modified={modified}
            extensions={extensions}
          />
        </div>
      </DialogContent>
    </Dialog>
  );
}
