/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import {
  DatabaseZap,
  Download,
  Info,
  RefreshCw,
  Search,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  deleteCacheEntry,
  fetchCacheDump,
  fetchCacheEntries,
  flushCache,
  loadCacheDump,
  type CacheEntryRow,
} from "@/lib/oxidns-api";
import type {
  PluginCardComponentProps,
  PluginComponentDefinition,
  PluginDetailComponentProps,
} from "../types";
import { DnsRecordDetailDialog } from "../dns-record-detail-dialog";
import { PluginCardTemplate } from "../plugin-card-template";
import {
  PluginDetailTemplate,
  PluginNotAppliedPlaceholder,
} from "../plugin-detail-template";
import { usePluginAppliedStatus } from "@/hooks/use-plugin-applied";
import { useI18n } from "@/lib/i18n/provider";
import { WEBUI } from "@/lib/i18n/keys";

function CachePluginCard({
  plugin,
  compact = false,
}: PluginCardComponentProps) {
  const { t } = useI18n();
  return (
    <PluginCardTemplate
      plugin={plugin}
      compact={compact}
      icon={<DatabaseZap className="h-4 w-4" />}
    >
      <div className="space-y-2 text-xs text-muted-foreground">
        <div>{t(WEBUI.cache.cardDescription)}</div>
        {!compact && (
          <div className="font-mono text-foreground">
            size={String(plugin.config.size ?? "default")}
          </div>
        )}
      </div>
    </PluginCardTemplate>
  );
}

function CachePluginDetail(props: PluginDetailComponentProps) {
  const { t } = useI18n();
  return (
    <PluginDetailTemplate
      {...props}
      icon={<DatabaseZap className="h-5 w-5" />}
      summaryItems={[
        {
          label: t(WEBUI.cache.capacityLabel),
          value: String(
            props.plugin.config.size ?? t(WEBUI.common.defaultValue),
          ),
        },
        {
          label: t(WEBUI.cache.negCacheLabel),
          value:
            props.plugin.config.cache_negative === false
              ? t(WEBUI.common.disabled)
              : t(WEBUI.common.enabled),
        },
        {
          label: t(WEBUI.cache.ecsKeyLabel),
          value: props.plugin.config.ecs_in_key
            ? t(WEBUI.common.enabled)
            : t(WEBUI.common.disabled),
        },
      ]}
      metricsContent={<CacheEntriesPanel tag={props.plugin.name} />}
    />
  );
}

function CacheEntriesPanel({ tag }: { tag: string }) {
  const appliedStatus = usePluginAppliedStatus(tag);
  if (appliedStatus === "not-applied") {
    return <PluginNotAppliedPlaceholder />;
  }
  return <CacheEntriesPanelInner tag={tag} />;
}

function CacheEntriesPanelInner({ tag }: { tag: string }) {
  const { locale, t } = useI18n();
  const [entries, setEntries] = useState<CacheEntryRow[]>([]);
  const [nextCursor, setNextCursor] = useState<string | undefined>();
  const [total, setTotal] = useState(0);
  const [selected, setSelected] = useState<CacheEntryRow | null>(null);
  const [qnameInput, setQnameInput] = useState("");
  const [appliedQname, setAppliedQname] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(
    async (cursor?: string, qname = "") => {
      setLoading(true);
      setError(null);
      try {
        const response = await fetchCacheEntries(tag, {
          limit: 100,
          cursor,
          qname: qname || undefined,
        });
        setEntries((current) =>
          cursor ? [...current, ...response.entries] : response.entries,
        );
        setNextCursor(response.next_cursor);
        setTotal(response.total_entries);
      } catch (err) {
        setError(
          err instanceof Error ? err.message : t(WEBUI.cache.readEntriesFailed),
        );
      } finally {
        setLoading(false);
      }
    },
    [tag, t],
  );

  useEffect(() => {
    const timer = window.setTimeout(() => void load(), 0);
    return () => window.clearTimeout(timer);
  }, [load]);

  const handleDelete = async (entry: CacheEntryRow) => {
    setError(null);
    try {
      await deleteCacheEntry(tag, entry.id);
      setEntries((current) => current.filter((item) => item.id !== entry.id));
      setTotal((current) => Math.max(0, current - 1));
      if (selected?.id === entry.id) {
        setSelected(null);
      }
    } catch (err) {
      setError(
        err instanceof Error ? err.message : t(WEBUI.cache.deleteEntryFailed),
      );
    }
  };

  const handleFlush = async () => {
    setError(null);
    try {
      await flushCache(tag);
      setEntries([]);
      setTotal(0);
      setNextCursor(undefined);
      setSelected(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : t(WEBUI.cache.flushFailed));
    }
  };

  const applyQnameFilter = () => {
    const nextQname = qnameInput.trim();
    setAppliedQname(nextQname);
    void load(undefined, nextQname);
  };

  const clearQnameFilter = () => {
    setQnameInput("");
    setAppliedQname("");
    void load(undefined, "");
  };

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader className="grid gap-3 p-4 pb-2 sm:grid-cols-[1fr_auto] sm:items-center">
          <div className="min-w-0">
            <CardTitle className="text-sm">
              {t(WEBUI.cache.entriesTitle)}
            </CardTitle>
            <div className="mt-2 flex flex-wrap gap-2 text-xs text-muted-foreground">
              <span className="rounded-full border bg-muted/30 px-2 py-0.5">
                {appliedQname
                  ? t(WEBUI.cache.matchCount, { total })
                  : t(WEBUI.cache.totalCount, { total })}
              </span>
              <span className="rounded-full border bg-muted/30 px-2 py-0.5">
                {t(WEBUI.cache.loadedCount, { count: entries.length })}
              </span>
              <span className="rounded-full border bg-muted/30 px-2 py-0.5">
                {t(WEBUI.metrics.fresh)}{" "}
                {entries.filter((entry) => entry.fresh).length}
              </span>
              <span className="rounded-full border bg-muted/30 px-2 py-0.5">
                {t(WEBUI.metrics.stale)}{" "}
                {entries.filter((entry) => entry.stale).length}
              </span>
            </div>
          </div>
          <div className="flex flex-wrap justify-end gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={loading}
              onClick={() => load(undefined, appliedQname)}
            >
              <RefreshCw className="h-4 w-4" />
              {t(WEBUI.common.refresh)}
            </Button>
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button variant="outline" size="sm" disabled={loading}>
                  <Trash2 className="h-4 w-4" />
                  {t(WEBUI.common.clear)}
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogMedia className="bg-destructive/10 text-destructive">
                    <Trash2 className="h-5 w-5" />
                  </AlertDialogMedia>
                  <AlertDialogTitle>
                    {t(WEBUI.cache.flushTitle)}
                  </AlertDialogTitle>
                  <AlertDialogDescription>
                    {t(WEBUI.cache.flushDesc, { tag })}
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>
                    {t(WEBUI.common.cancel)}
                  </AlertDialogCancel>
                  <AlertDialogAction
                    variant="destructive"
                    onClick={() => void handleFlush()}
                  >
                    {t(WEBUI.common.clear)}
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          </div>
        </CardHeader>
        <CardContent className="p-4 pt-0">
          {error && (
            <div className="mb-3 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}
          <form
            className="mb-3 flex flex-col gap-2 rounded-md border bg-muted/20 p-3 sm:flex-row sm:items-end"
            onSubmit={(event) => {
              event.preventDefault();
              applyQnameFilter();
            }}
          >
            <label className="grid min-w-0 flex-1 gap-1 text-xs text-muted-foreground">
              {t(WEBUI.cache.qnameFilterLabel)}
              <Input
                value={qnameInput}
                onChange={(event) => setQnameInput(event.target.value)}
                placeholder="example.com"
                className="h-8 font-mono text-sm"
              />
            </label>
            <div className="flex gap-2">
              <Button
                type="submit"
                variant="outline"
                size="sm"
                disabled={loading}
              >
                <Search className="h-4 w-4" />
                {t(WEBUI.cache.filterButton)}
              </Button>
              {appliedQname && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  disabled={loading}
                  onClick={clearQnameFilter}
                >
                  <X className="h-4 w-4" />
                  {t(WEBUI.cache.clearFilter)}
                </Button>
              )}
            </div>
          </form>
          <div className="overflow-hidden rounded-md border">
            <Table className="min-w-[820px]">
              <TableHeader>
                <TableRow className="bg-muted/30 hover:bg-muted/30">
                  <TableHead>{t(WEBUI.cache.cacheKey)}</TableHead>
                  <TableHead>{t(WEBUI.cache.statusHeader)}</TableHead>
                  <TableHead>
                    <span className="inline-flex items-center gap-1">
                      TTL
                      <Popover>
                        <PopoverTrigger asChild>
                          <button
                            type="button"
                            className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none"
                            aria-label={t(WEBUI.cache.ttlInfoAria)}
                          >
                            <Info className="h-3.5 w-3.5" />
                          </button>
                        </PopoverTrigger>
                        <PopoverContent
                          side="top"
                          className="w-auto max-w-[16rem] p-2 text-xs"
                        >
                          {t(WEBUI.cache.ttlInfoTooltip)}
                        </PopoverContent>
                      </Popover>
                    </span>
                  </TableHead>
                  <TableHead>RCODE</TableHead>
                  <TableHead>
                    <span className="inline-flex items-center gap-1">
                      {t(WEBUI.cache.answerHeader)}
                      <Popover>
                        <PopoverTrigger asChild>
                          <button
                            type="button"
                            className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none"
                            aria-label={t(WEBUI.cache.answerInfoAria)}
                          >
                            <Info className="h-3.5 w-3.5" />
                          </button>
                        </PopoverTrigger>
                        <PopoverContent
                          side="top"
                          className="w-auto max-w-[16rem] p-2 text-xs"
                        >
                          {t(WEBUI.dnsRecord.responseRecordsTooltip)}
                        </PopoverContent>
                      </Popover>
                    </span>
                  </TableHead>
                  <TableHead>{t(WEBUI.cache.lastAccess)}</TableHead>
                  <TableHead className="w-16" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {entries.map((entry) => (
                  <TableRow
                    key={entry.id}
                    className="cursor-pointer"
                    onClick={() => setSelected(entry)}
                  >
                    <TableCell className="max-w-[24rem]">
                      <div className="flex min-w-0 items-center gap-2">
                        <span
                          className="truncate font-mono"
                          title={`${entry.domain} ${entry.record_type}`}
                        >
                          {entry.domain}
                        </span>
                        <Badge variant="outline" className="font-mono">
                          {entry.record_type}
                        </Badge>
                      </div>
                    </TableCell>
                    <TableCell>{cacheStatusBadge(entry, t)}</TableCell>
                    <TableCell className="font-mono">
                      <div className="flex items-baseline gap-1">
                        <span>{entry.remaining_ttl}s</span>
                        <span className="text-xs text-muted-foreground">
                          / {entry.ttl}s
                        </span>
                      </div>
                    </TableCell>
                    <TableCell>{rcodeBadge(entry.rcode)}</TableCell>
                    <TableCell>
                      <div className="flex items-center gap-1 font-mono text-xs">
                        <span>{entry.answer_count}</span>
                        <span className="text-muted-foreground">
                          /{" "}
                          {entry.authority_count ??
                            entry.authorities_json?.length ??
                            0}
                          /{" "}
                          {entry.additional_count ??
                            entry.additionals_json?.length ??
                            0}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell className="font-mono text-xs text-muted-foreground">
                      <div className="flex min-w-0 items-center gap-2">
                        <span
                          title={formatCacheFullTime(
                            entry.last_access_unix_ms,
                            entry.last_access_ms,
                            locale,
                          )}
                        >
                          {formatCacheShortTime(
                            entry.last_access_unix_ms,
                            entry.last_access_ms,
                            locale,
                          )}
                        </span>
                        {entry.ecs_scope && (
                          <Badge variant="outline" className="font-mono">
                            ECS {entry.ecs_scope.family}/
                            {entry.ecs_scope.source_prefix}
                          </Badge>
                        )}
                      </div>
                    </TableCell>
                    <TableCell
                      onClick={(event) => event.stopPropagation()}
                      onPointerDown={(event) => event.stopPropagation()}
                    >
                      <AlertDialog>
                        <AlertDialogTrigger asChild>
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            onClick={(event) => event.stopPropagation()}
                            aria-label={t(WEBUI.cache.deleteEntryAria)}
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </AlertDialogTrigger>
                        <AlertDialogContent>
                          <AlertDialogHeader>
                            <AlertDialogMedia className="bg-destructive/10 text-destructive">
                              <Trash2 className="h-5 w-5" />
                            </AlertDialogMedia>
                            <AlertDialogTitle>
                              {t(WEBUI.cache.deleteEntryTitle)}
                            </AlertDialogTitle>
                            <AlertDialogDescription>
                              {t(WEBUI.cache.deleteEntryDesc, {
                                domain: entry.domain,
                                recordType: entry.record_type,
                              })}
                            </AlertDialogDescription>
                          </AlertDialogHeader>
                          <AlertDialogFooter>
                            <AlertDialogCancel>
                              {t(WEBUI.common.cancel)}
                            </AlertDialogCancel>
                            <AlertDialogAction
                              variant="destructive"
                              onClick={(event) => {
                                event.stopPropagation();
                                void handleDelete(entry);
                              }}
                            >
                              {t(WEBUI.common.delete)}
                            </AlertDialogAction>
                          </AlertDialogFooter>
                        </AlertDialogContent>
                      </AlertDialog>
                    </TableCell>
                  </TableRow>
                ))}
                {!entries.length && (
                  <TableRow>
                    <TableCell
                      colSpan={7}
                      className="h-24 text-center text-muted-foreground"
                    >
                      {loading
                        ? t(WEBUI.cache.loadingEntries)
                        : t(WEBUI.cache.noEntries)}
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>
          {nextCursor && (
            <Button
              variant="outline"
              size="sm"
              className="mt-3"
              disabled={loading}
              onClick={() => load(nextCursor, appliedQname)}
            >
              {t(WEBUI.common.loadMore)}
            </Button>
          )}
        </CardContent>
        <CacheEntryDetailDialog
          entry={selected}
          onClose={() => setSelected(null)}
        />
      </Card>

      <CacheMaintenancePanel tag={tag} />
    </div>
  );
}

function CacheMaintenancePanel({ tag }: { tag: string }) {
  const { t } = useI18n();
  const [dumpLoading, setDumpLoading] = useState(false);
  const [loadLoading, setLoadLoading] = useState(false);
  const [loadResult, setLoadResult] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleDump = async () => {
    setDumpLoading(true);
    setError(null);
    try {
      const blob = await fetchCacheDump(tag);
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${tag}.dump`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : t(WEBUI.cache.exportFailed),
      );
    } finally {
      setDumpLoading(false);
    }
  };

  const handleLoadDump = async (file: File) => {
    setLoadLoading(true);
    setError(null);
    setLoadResult(null);
    try {
      const buffer = await file.arrayBuffer();
      const result = await loadCacheDump(tag, buffer);
      setLoadResult(result.loaded_entries);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : t(WEBUI.cache.importFailed),
      );
    } finally {
      setLoadLoading(false);
      if (fileInputRef.current) fileInputRef.current.value = "";
    }
  };

  return (
    <Card>
      <CardHeader className="p-4 pb-2">
        <CardTitle className="text-sm">
          {t(WEBUI.cache.maintenanceTitle)}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3 p-4 pt-0">
        {error && (
          <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}
        {loadResult !== null && (
          <div className="rounded-md border border-green-500/30 bg-green-500/10 px-3 py-2 text-sm text-green-600 dark:text-green-400">
            {t(WEBUI.cache.loadedResult, { count: loadResult })}
          </div>
        )}
        <div className="flex items-center gap-3">
          <Button
            variant="outline"
            size="sm"
            disabled={dumpLoading}
            onClick={() => void handleDump()}
          >
            <Download className="h-4 w-4" />
            {t(WEBUI.cache.exportButton)}
          </Button>
          <span className="text-xs text-muted-foreground">
            {dumpLoading
              ? t(WEBUI.cache.exportingStatus)
              : t(WEBUI.cache.exportDesc)}
          </span>
        </div>
        <div className="flex items-center gap-3">
          <Button
            variant="outline"
            size="sm"
            disabled={loadLoading}
            onClick={() => fileInputRef.current?.click()}
          >
            <Upload className="h-4 w-4" />
            {t(WEBUI.cache.importButton)}
          </Button>
          <span className="text-xs text-muted-foreground">
            {loadLoading
              ? t(WEBUI.cache.importingStatus)
              : t(WEBUI.cache.importDesc)}
          </span>
          <input
            ref={fileInputRef}
            type="file"
            accept=".dump,application/octet-stream"
            className="hidden"
            onChange={(e) => {
              const file = e.target.files?.[0];
              if (file) void handleLoadDump(file);
            }}
          />
        </div>
      </CardContent>
    </Card>
  );
}

function CacheEntryDetailDialog({
  entry,
  onClose,
}: {
  entry: CacheEntryRow | null;
  onClose: () => void;
}) {
  const { locale, t } = useI18n();
  return (
    <DnsRecordDetailDialog
      open={Boolean(entry)}
      onOpenChange={(open) => !open && onClose()}
      title={
        entry
          ? `${entry.domain} ${entry.record_type}`
          : t(WEBUI.cache.detailTitle)
      }
      subtitle={
        entry
          ? t(WEBUI.cache.detailSubtitle, {
              time: formatCacheFullTime(
                entry.cache_time_unix_ms,
                entry.cache_time_ms,
                locale,
              ),
            })
          : undefined
      }
      status={entry ? cacheStatusBadge(entry, t) : undefined}
      summaryItems={
        entry
          ? [
              {
                label: t(WEBUI.cache.domainLabel),
                value: entry.domain,
                title: entry.domain,
                mono: true,
                wide: true,
              },
              {
                label: t(WEBUI.cache.recordTypeLabel),
                value: entry.record_type,
                mono: true,
              },
              {
                label: t(WEBUI.cache.recordClassLabel),
                value: entry.dns_class,
                mono: true,
              },
              { label: "RCODE", value: entry.rcode, mono: true },
              { label: "TTL", value: `${entry.ttl}s`, mono: true },
              {
                label: t(WEBUI.cache.remainingTtlLabel),
                value: `${entry.remaining_ttl}s`,
                mono: true,
              },
              {
                label: (
                  <span className="inline-flex items-center gap-1">
                    {t(WEBUI.dnsRecord.responseRecordsLabel)}
                    <Popover>
                      <PopoverTrigger asChild>
                        <button
                          type="button"
                          className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none"
                          aria-label={t(WEBUI.dnsRecord.responseRecordsAria)}
                        >
                          <Info className="h-3.5 w-3.5" />
                        </button>
                      </PopoverTrigger>
                      <PopoverContent
                        side="top"
                        className="w-auto max-w-[16rem] p-2 text-xs"
                      >
                        {t(WEBUI.dnsRecord.responseRecordsTooltip)}
                      </PopoverContent>
                    </Popover>
                  </span>
                ),
                value: `${entry.answer_count} / ${entry.authority_count ?? entry.authorities_json?.length ?? 0} / ${entry.additional_count ?? entry.additionals_json?.length ?? 0}`,
                mono: true,
              },
              {
                label: t(WEBUI.cache.cacheFlagsLabel),
                value: `DO=${entry.do_bit ? "1" : "0"} CD=${entry.cd_bit ? "1" : "0"}`,
                mono: true,
              },
              {
                label: t(WEBUI.cache.cacheTimeLabel),
                value: formatCacheFullTime(
                  entry.cache_time_unix_ms,
                  entry.cache_time_ms,
                  locale,
                ),
                title: `runtime +${entry.cache_time_ms}ms`,
                mono: true,
                wide: true,
              },
              {
                label: t(WEBUI.cache.expireAtLabel),
                value: formatCacheFullTime(
                  entry.expire_at_unix_ms,
                  entry.expire_at_ms,
                  locale,
                ),
                title: `runtime +${entry.expire_at_ms}ms`,
                mono: true,
                wide: true,
              },
              {
                label: t(WEBUI.cache.lastAccess),
                value: formatCacheFullTime(
                  entry.last_access_unix_ms,
                  entry.last_access_ms,
                  locale,
                ),
                title: `runtime +${entry.last_access_ms}ms`,
                mono: true,
                wide: true,
              },
            ]
          : []
      }
      questions={
        entry
          ? [
              {
                name: entry.domain,
                qclass: entry.dns_class,
                qtype: entry.record_type,
              },
            ]
          : []
      }
      sections={
        entry
          ? [
              {
                title: t(WEBUI.dnsRecord.answerSection),
                records: entry.answers_json ?? [],
                emptyLabel: t(WEBUI.dnsRecord.emptyAnswer),
              },
              {
                title: t(WEBUI.dnsRecord.authoritySection),
                records: entry.authorities_json ?? [],
                emptyLabel: t(WEBUI.dnsRecord.emptyAuthority),
              },
              {
                title: t(WEBUI.dnsRecord.additionalSection),
                records: entry.additionals_json ?? [],
                emptyLabel: t(WEBUI.dnsRecord.emptyAdditional),
              },
              {
                title: t(WEBUI.dnsRecord.signatureSection),
                records: entry.signature_json ?? [],
                emptyLabel: t(WEBUI.dnsRecord.emptySignature),
              },
            ]
          : []
      }
      blocks={
        entry
          ? [
              {
                title: t(WEBUI.cache.cacheKey),
                children: (
                  <div className="break-all font-mono text-xs text-muted-foreground">
                    {entry.id}
                  </div>
                ),
              },
              ...(entry.ecs_scope
                ? [
                    {
                      title: t(WEBUI.cache.ecsScopeBlock),
                      children: (
                        <div className="grid gap-2 font-mono text-xs text-muted-foreground sm:grid-cols-2">
                          <span>family={entry.ecs_scope.family}</span>
                          <span>source={entry.ecs_scope.source_prefix}</span>
                          <span>scope={entry.ecs_scope.scope_prefix}</span>
                          <span className="break-all">
                            network={entry.ecs_scope.network_hex}
                          </span>
                        </div>
                      ),
                    },
                  ]
                : []),
            ]
          : []
      }
    />
  );
}

type TFn = (
  key: string,
  params?: Record<string, string | number | boolean | null | undefined>,
) => string;

function cacheStatusBadge(entry: CacheEntryRow, t: TFn) {
  if (entry.fresh) {
    return <Badge variant="secondary">{t(WEBUI.metrics.fresh)}</Badge>;
  }
  if (entry.stale) {
    return <Badge variant="outline">{t(WEBUI.metrics.stale)}</Badge>;
  }
  return <Badge variant="destructive">{t(WEBUI.cache.expiredBadge)}</Badge>;
}

function rcodeBadge(rcode: string) {
  if (rcode?.toLowerCase() === "no error") {
    return <Badge variant="secondary">No Error</Badge>;
  }
  return <Badge variant="outline">{rcode}</Badge>;
}

function formatCacheShortTime(
  ms: number | undefined,
  runtimeMs: number | undefined,
  locale: ReturnType<typeof useI18n>["locale"],
) {
  if (typeof ms !== "number") {
    return typeof runtimeMs === "number" ? formatRuntimeMs(runtimeMs) : "-";
  }
  return new Date(ms).toLocaleString(locale, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function formatCacheFullTime(
  ms: number | undefined,
  runtimeMs: number | undefined,
  locale: ReturnType<typeof useI18n>["locale"],
) {
  if (typeof ms !== "number") {
    return typeof runtimeMs === "number" ? formatRuntimeMs(runtimeMs) : "-";
  }
  return new Date(ms).toLocaleString(locale, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function formatRuntimeMs(ms: number) {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const days = Math.floor(totalSeconds / 86_400);
  const hours = Math.floor((totalSeconds % 86_400) / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const seconds = totalSeconds % 60;

  if (days > 0) {
    return `+${days}d ${hours}h`;
  }
  if (hours > 0) {
    return `+${hours}h ${minutes}m`;
  }
  if (minutes > 0) {
    return `+${minutes}m ${seconds}s`;
  }
  return `+${seconds}s`;
}

export const cachePlugin: PluginComponentDefinition = {
  Card: CachePluginCard,
  Detail: CachePluginDetail,
};
