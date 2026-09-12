"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { Download, Loader2, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { usePluginAppliedStatus } from "@/hooks/use-plugin-applied";
import { useAppStore } from "@/lib/store";
import { useAuthStore } from "@/lib/auth-store";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import {
  fetchDownloads,
  runDownload,
  type DownloadItem,
  type DownloadResult,
} from "@/lib/oxidns-api";
import {
  PluginDetailTemplate,
  PluginNotAppliedPlaceholder,
} from "../plugin-detail-template";
import type {
  PluginComponentDefinition,
  PluginDetailComponentProps,
} from "../types";

function DownloadDetail(props: PluginDetailComponentProps) {
  const runningVersion = useAppStore((s) => s.runningVersion);
  const epoch = useAuthStore((s) => s.connectionEpoch);
  return (
    <PluginDetailTemplate
      {...props}
      runtimeContent={
        <DownloadsPanel
          key={`${props.plugin.name}:${epoch}:${runningVersion}`}
          tag={props.plugin.name}
        />
      }
    />
  );
}

function DownloadsPanel({ tag }: { tag: string }) {
  const applied = usePluginAppliedStatus(tag);
  if (applied === "not-applied") return <PluginNotAppliedPlaceholder />;
  return <DownloadsPanelInner tag={tag} />;
}

function DownloadsPanelInner({ tag }: { tag: string }) {
  const { t } = useI18n();
  const [items, setItems] = useState<DownloadItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [running, setRunning] = useState(false);
  const [pending, setPending] = useState<number | "all" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<DownloadResult | null>(null);
  const active = useRef(true);
  const requestPending = useRef(false);
  const loadController = useRef<AbortController | null>(null);

  const load = useCallback(async () => {
    loadController.current?.abort();
    const controller = new AbortController();
    loadController.current = controller;
    setLoading(true);
    try {
      const response = await fetchDownloads(tag, controller.signal);
      if (controller.signal.aborted) return;
      setItems(response.downloads);
      setRunning(response.running);
      setError(null);
    } catch (err) {
      if (controller.signal.aborted) return;
      setItems([]);
      setError(
        err instanceof Error ? err.message : t(WEBUI.download.loadFailed),
      );
    } finally {
      if (!controller.signal.aborted) setLoading(false);
    }
  }, [tag, t]);

  useEffect(() => {
    active.current = true;
    const timer = setTimeout(() => void load(), 0);
    return () => {
      clearTimeout(timer);
      active.current = false;
      loadController.current?.abort();
    };
  }, [load]);

  useEffect(() => {
    if (!running || pending !== null) return;
    const timer = setTimeout(() => void load(), 1500);
    return () => clearTimeout(timer);
  }, [running, pending, load, loading]);

  const download = async (index?: number) => {
    if (requestPending.current || running || loading) return;
    requestPending.current = true;
    setPending(index ?? "all");
    setError(null);
    setResult(null);
    try {
      const response = await runDownload(tag, index);
      if (active.current) setResult(response);
    } catch (err) {
      if (active.current)
        setError(
          err instanceof Error ? err.message : t(WEBUI.download.runFailed),
        );
    } finally {
      requestPending.current = false;
      if (active.current) setPending(null);
    }
  };

  const busy = pending !== null || running;
  return (
    <Card className="mb-4 shrink-0">
      <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2 p-4 pb-2">
        <CardTitle className="text-sm">{t(WEBUI.download.title)}</CardTitle>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={loading || pending !== null}
            onClick={() => void load()}
          >
            <RefreshCw className="h-4 w-4" />
            {t(WEBUI.download.retry)}
          </Button>
          <Button
            size="sm"
            disabled={busy || loading || items.length === 0}
            onClick={() => void download()}
          >
            {pending === "all" ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Download className="h-4 w-4" />
            )}
            {pending === "all"
              ? t(WEBUI.download.downloading)
              : t(WEBUI.download.downloadAll)}
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-3 p-4 pt-0">
        <p className="text-xs text-muted-foreground">
          {t(WEBUI.download.runtimeHint)}
        </p>
        {error && (
          <p role="alert" className="break-words text-sm text-destructive">
            {error}
          </p>
        )}
        {result && (
          <p
            role="status"
            className={
              result.ok ? "text-sm text-primary" : "text-sm text-destructive"
            }
          >
            {result.ok
              ? t(WEBUI.download.success, { count: result.succeeded })
              : t(WEBUI.download.partialFailure, {
                  succeeded: result.succeeded,
                  failed: result.failed,
                })}
          </p>
        )}
        {running && (
          <p role="status" className="text-xs text-muted-foreground">
            {t(WEBUI.download.busy)}
          </p>
        )}
        {loading && items.length === 0 ? (
          <p role="status" className="text-sm text-muted-foreground">
            {t(WEBUI.download.loading)}
          </p>
        ) : items.length === 0 && !error ? (
          <p className="text-sm text-muted-foreground">
            {t(WEBUI.download.empty)}
          </p>
        ) : null}
        <div className="divide-y rounded-md border">
          {items.map((item) => (
            <div
              key={item.index}
              className="flex flex-wrap items-center gap-3 p-3"
            >
              <div className="min-w-0 flex-1 basis-48 space-y-1 font-mono text-xs">
                <div className="break-all">{item.url}</div>
                <div className="break-all text-muted-foreground">
                  {item.path}
                </div>
              </div>
              <Button
                variant="outline"
                size="sm"
                disabled={busy || loading}
                onClick={() => void download(item.index)}
                aria-label={`${t(WEBUI.download.download)} ${item.path}`}
              >
                {pending === item.index ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Download className="h-4 w-4" />
                )}
                {pending === item.index
                  ? t(WEBUI.download.downloading)
                  : t(WEBUI.download.download)}
              </Button>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  );
}

export const downloadPlugin: PluginComponentDefinition = {
  Detail: DownloadDetail,
};
