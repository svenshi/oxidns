"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Database,
  Download,
  History,
  Loader2,
  RefreshCw,
  RotateCcw,
  Search,
  Trash2,
  Upload,
} from "lucide-react";
import { AppHeader } from "@/components/shell/app-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import {
  deleteCacheEntry,
  fetchCacheDump,
  fetchCacheEntries,
  fetchStandardHistory,
  fetchStandardHistoryRestore,
  flushCache,
  loadCacheDump,
  type CacheEntryRow,
  type StandardHistoryItem,
} from "@/lib/oxidns-api";
import { useAppStore } from "@/lib/store";

export default function StandardOperationsPage() {
  const settings = useAppStore((state) => state.standardSettings);
  const lastGenerated = useAppStore((state) => state.standardLastGenerated);
  const saveStandardSettings = useAppStore(
    (state) => state.saveStandardSettings,
  );
  const isApplying = useAppStore((state) => state.isApplying);
  const { t, formatDateTime, formatNumber } = useI18n();
  const cacheTags = useMemo(
    () => lastGenerated?.tagMap.caches ?? {},
    [lastGenerated],
  );
  const cachePaths = useMemo(
    () =>
      settings.paths
        .filter((path) => Boolean(cacheTags[path.id]))
        .map((path) => ({
          ...path,
          tag: cacheTags[path.id],
        })),
    [cacheTags, settings.paths],
  );
  const [selectedPathId, setSelectedPathId] = useState(
    () => cachePaths[0]?.id ?? "",
  );
  const selectedCache =
    cachePaths.find((path) => path.id === selectedPathId) ?? cachePaths[0];
  const [cacheEntries, setCacheEntries] = useState<CacheEntryRow[]>([]);
  const [cacheTotal, setCacheTotal] = useState(0);
  const [cacheQuery, setCacheQuery] = useState("");
  const [cacheLoading, setCacheLoading] = useState(false);
  const [cacheAction, setCacheAction] = useState<string | null>(null);
  const [history, setHistory] = useState<StandardHistoryItem[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [restoringId, setRestoringId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const loadCache = useCallback(async () => {
    if (!selectedCache) {
      setCacheEntries([]);
      setCacheTotal(0);
      return;
    }
    setCacheLoading(true);
    setError(null);
    try {
      const response = await fetchCacheEntries(selectedCache.tag, {
        limit: 100,
        qname: cacheQuery.trim() || undefined,
      });
      setCacheEntries(response.entries);
      setCacheTotal(response.total_entries);
    } catch (loadError) {
      setError(
        loadError instanceof Error ? loadError.message : String(loadError),
      );
    } finally {
      setCacheLoading(false);
    }
  }, [cacheQuery, selectedCache]);

  const loadHistory = useCallback(async () => {
    setHistoryLoading(true);
    setError(null);
    try {
      setHistory((await fetchStandardHistory()).entries);
    } catch (loadError) {
      setError(
        loadError instanceof Error ? loadError.message : String(loadError),
      );
    } finally {
      setHistoryLoading(false);
    }
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void loadCache();
      void loadHistory();
    }, 0);
    return () => window.clearTimeout(timer);
    // Initial status load; later loads are explicit to avoid searching per keypress.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedCache?.tag]);

  const clearCache = async () => {
    if (
      !selectedCache ||
      !window.confirm(
        t(WEBUI.standardOperations.cacheClearConfirm, {
          path: selectedCache.name || selectedCache.id,
        }),
      )
    ) {
      return;
    }
    await runCacheAction("clear", async () => {
      await flushCache(selectedCache.tag);
      await loadCache();
      setMessage(t(WEBUI.standardOperations.cacheClearSuccess));
    });
  };

  const exportCache = async () => {
    if (!selectedCache) return;
    await runCacheAction("dump", async () => {
      const blob = await fetchCacheDump(selectedCache.tag);
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `${selectedCache.tag}.dump`;
      anchor.click();
      URL.revokeObjectURL(url);
      setMessage(t(WEBUI.standardOperations.cacheDumpSuccess));
    });
  };

  const importCache = async (file: File) => {
    if (!selectedCache) return;
    await runCacheAction("load", async () => {
      const response = await loadCacheDump(
        selectedCache.tag,
        await file.arrayBuffer(),
      );
      await loadCache();
      setMessage(
        t(WEBUI.standardOperations.cacheLoadSuccess, {
          count: formatNumber(response.loaded_entries),
        }),
      );
    });
  };

  const removeEntry = async (entry: CacheEntryRow) => {
    if (!selectedCache) return;
    await runCacheAction(`entry:${entry.id}`, async () => {
      await deleteCacheEntry(selectedCache.tag, entry.id);
      await loadCache();
    });
  };

  const runCacheAction = async (
    action: string,
    operation: () => Promise<void>,
  ) => {
    setCacheAction(action);
    setError(null);
    setMessage(null);
    try {
      await operation();
    } catch (actionError) {
      setError(
        actionError instanceof Error
          ? actionError.message
          : String(actionError),
      );
    } finally {
      setCacheAction(null);
    }
  };

  const restoreHistory = async (item: StandardHistoryItem) => {
    if (
      !window.confirm(
        t(WEBUI.standardOperations.historyRestoreConfirm, {
          time: formatDateTime(item.created_at_ms),
        }),
      )
    ) {
      return;
    }
    setRestoringId(item.id);
    setError(null);
    setMessage(null);
    try {
      const response = await fetchStandardHistoryRestore(item.id);
      await saveStandardSettings(response.entry.settings, { apply: true });
      setMessage(t(WEBUI.standardOperations.historyRestoreSuccess));
      await loadHistory();
    } catch (restoreError) {
      setError(
        restoreError instanceof Error
          ? restoreError.message
          : String(restoreError),
      );
    } finally {
      setRestoringId(null);
    }
  };

  return (
    <>
      <AppHeader title={t(WEBUI.standardOperations.title)} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-7xl space-y-6">
          <div>
            <h1 className="text-xl font-semibold tracking-tight">
              {t(WEBUI.standardOperations.title)}
            </h1>
            <p className="mt-1 text-sm text-muted-foreground">
              {t(WEBUI.standardOperations.description)}
            </p>
          </div>

          {error ? (
            <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
              {error}
            </div>
          ) : null}
          {message ? (
            <div className="rounded-lg border border-primary/30 bg-primary/5 p-3 text-sm text-primary">
              {message}
            </div>
          ) : null}

          <Card>
            <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3 space-y-0">
              <div>
                <CardTitle className="flex items-center gap-2 text-base">
                  <Database className="size-4" />
                  {t(WEBUI.standardOperations.cacheTitle)}
                </CardTitle>
                <p className="mt-1 text-sm text-muted-foreground">
                  {t(WEBUI.standardOperations.cacheDescription)}
                </p>
              </div>
              <div className="flex flex-wrap justify-end gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={!selectedCache || cacheAction !== null}
                  onClick={() => void exportCache()}
                >
                  {cacheAction === "dump" ? (
                    <Loader2 className="size-4 animate-spin" />
                  ) : (
                    <Download className="size-4" />
                  )}
                  {t(WEBUI.standardOperations.cacheDump)}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={!selectedCache || cacheAction !== null}
                  onClick={() => fileInputRef.current?.click()}
                >
                  {cacheAction === "load" ? (
                    <Loader2 className="size-4 animate-spin" />
                  ) : (
                    <Upload className="size-4" />
                  )}
                  {t(WEBUI.standardOperations.cacheLoad)}
                </Button>
                <input
                  ref={fileInputRef}
                  type="file"
                  className="hidden"
                  onChange={(event) => {
                    const file = event.target.files?.[0];
                    if (file) void importCache(file);
                    event.currentTarget.value = "";
                  }}
                />
                <Button
                  type="button"
                  variant="destructive"
                  size="sm"
                  disabled={!selectedCache || cacheAction !== null}
                  onClick={() => void clearCache()}
                >
                  {cacheAction === "clear" ? (
                    <Loader2 className="size-4 animate-spin" />
                  ) : (
                    <Trash2 className="size-4" />
                  )}
                  {t(WEBUI.standardOperations.cacheClear)}
                </Button>
              </div>
            </CardHeader>
            <CardContent className="space-y-4">
              {cachePaths.length === 0 ? (
                <div className="rounded-lg border border-dashed p-6 text-sm text-muted-foreground">
                  {t(WEBUI.standardOperations.cacheUnavailable)}
                </div>
              ) : (
                <>
                  <div className="grid gap-3 md:grid-cols-[240px_minmax(0,1fr)_auto]">
                    <Select
                      value={selectedCache?.id}
                      onValueChange={setSelectedPathId}
                    >
                      <SelectTrigger className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {cachePaths.map((path) => (
                          <SelectItem key={path.id} value={path.id}>
                            {path.name || path.id}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <div className="relative">
                      <Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
                      <Input
                        className="pl-8 font-mono"
                        value={cacheQuery}
                        placeholder="example.com"
                        onChange={(event) => setCacheQuery(event.target.value)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") void loadCache();
                        }}
                      />
                    </div>
                    <Button
                      type="button"
                      variant="outline"
                      disabled={cacheLoading}
                      onClick={() => void loadCache()}
                    >
                      {cacheLoading ? (
                        <Loader2 className="size-4 animate-spin" />
                      ) : (
                        <RefreshCw className="size-4" />
                      )}
                      {t(WEBUI.standardOperations.refresh)}
                    </Button>
                  </div>
                  <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
                    <Badge variant="secondary">{selectedCache?.tag}</Badge>
                    <Badge variant="outline">
                      {t(WEBUI.standardOperations.cacheCount, {
                        count: formatNumber(cacheTotal),
                      })}
                    </Badge>
                  </div>
                  <div className="overflow-hidden rounded-md border">
                    <Table className="min-w-[760px]">
                      <TableHeader>
                        <TableRow>
                          <TableHead>
                            {t(WEBUI.standardOperations.domain)}
                          </TableHead>
                          <TableHead>QTYPE</TableHead>
                          <TableHead>RCODE</TableHead>
                          <TableHead>TTL</TableHead>
                          <TableHead>
                            {t(WEBUI.standardOperations.state)}
                          </TableHead>
                          <TableHead className="text-right">
                            {t(WEBUI.standardOperations.action)}
                          </TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {cacheEntries.map((entry) => (
                          <TableRow key={entry.id}>
                            <TableCell className="font-mono text-xs">
                              {entry.domain}
                            </TableCell>
                            <TableCell>{entry.record_type}</TableCell>
                            <TableCell>{entry.rcode}</TableCell>
                            <TableCell>
                              {formatNumber(entry.remaining_ttl)}
                            </TableCell>
                            <TableCell>
                              <Badge
                                variant={entry.fresh ? "default" : "secondary"}
                              >
                                {entry.fresh
                                  ? t(WEBUI.standardOperations.cacheFresh)
                                  : t(WEBUI.standardOperations.cacheStale)}
                              </Badge>
                            </TableCell>
                            <TableCell className="text-right">
                              <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                disabled={cacheAction !== null}
                                onClick={() => void removeEntry(entry)}
                              >
                                {cacheAction === `entry:${entry.id}` ? (
                                  <Loader2 className="size-4 animate-spin" />
                                ) : (
                                  <Trash2 className="size-4" />
                                )}
                              </Button>
                            </TableCell>
                          </TableRow>
                        ))}
                        {cacheEntries.length === 0 ? (
                          <TableRow>
                            <TableCell colSpan={6} className="h-24 text-center">
                              {t(WEBUI.standardOperations.cacheEmpty)}
                            </TableCell>
                          </TableRow>
                        ) : null}
                      </TableBody>
                    </Table>
                  </div>
                </>
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
              <div>
                <CardTitle className="flex items-center gap-2 text-base">
                  <History className="size-4" />
                  {t(WEBUI.standardOperations.historyTitle)}
                </CardTitle>
                <p className="mt-1 text-sm text-muted-foreground">
                  {t(WEBUI.standardOperations.historyDescription)}
                </p>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={historyLoading}
                onClick={() => void loadHistory()}
              >
                {historyLoading ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : (
                  <RefreshCw className="size-4" />
                )}
                {t(WEBUI.standardOperations.refresh)}
              </Button>
            </CardHeader>
            <CardContent className="space-y-3">
              {history.map((item) => {
                const current =
                  item.config_version === lastGenerated?.configVersion;
                return (
                  <div
                    key={item.id}
                    className="flex flex-wrap items-center justify-between gap-3 rounded-lg border p-4"
                  >
                    <div className="min-w-0 space-y-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-medium">
                          {formatDateTime(item.created_at_ms)}
                        </span>
                        {current ? (
                          <Badge>
                            {t(WEBUI.standardOperations.historyCurrent)}
                          </Badge>
                        ) : null}
                        <Badge variant="outline">
                          schema {item.settings_schema}
                        </Badge>
                      </div>
                      <p className="font-mono text-xs text-muted-foreground">
                        {item.config_version.slice(0, 16)} ·{" "}
                        {item.transaction_id}
                      </p>
                      <p className="text-xs text-muted-foreground">
                        {t(WEBUI.standardOperations.historySummary, {
                          groups: formatNumber(item.upstream_group_count),
                          paths: formatNumber(item.path_count),
                        })}
                      </p>
                    </div>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={current || isApplying || restoringId !== null}
                      onClick={() => void restoreHistory(item)}
                    >
                      {restoringId === item.id ? (
                        <Loader2 className="size-4 animate-spin" />
                      ) : (
                        <RotateCcw className="size-4" />
                      )}
                      {t(WEBUI.standardOperations.historyRestore)}
                    </Button>
                  </div>
                );
              })}
              {!historyLoading && history.length === 0 ? (
                <div className="rounded-lg border border-dashed p-6 text-sm text-muted-foreground">
                  {t(WEBUI.standardOperations.historyEmpty)}
                </div>
              ) : null}
            </CardContent>
          </Card>
        </div>
      </main>
    </>
  );
}
