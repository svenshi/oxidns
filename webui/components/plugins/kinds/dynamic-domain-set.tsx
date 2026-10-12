/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { List, Loader2, Plus, RefreshCw, Search, Trash2 } from "lucide-react";
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
import { Textarea } from "@/components/ui/textarea";
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
  appendDynamicDomainRules,
  clearDynamicDomainRules,
  listDynamicDomainRules,
  removeDynamicDomainRules,
  type DynamicDomainRuleKind,
} from "@/lib/oxidns-api";
import type {
  PluginComponentDefinition,
  PluginDetailComponentProps,
} from "../types";
import {
  PluginDetailTemplate,
  PluginNotAppliedPlaceholder,
} from "../plugin-detail-template";
import { usePluginAppliedStatus } from "@/hooks/use-plugin-applied";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

const PAGE_LIMIT = 200;

function DynamicDomainSetDetail(props: PluginDetailComponentProps) {
  const { t } = useI18n();
  const config = props.plugin.config as Record<string, unknown>;
  const path = typeof config.path === "string" ? config.path : "-";
  const bootstrap = Array.isArray(config.bootstrap_rules)
    ? t(WEBUI.dynamicDomainSet.rulesCountValue, {
        count: config.bootstrap_rules.length,
      })
    : t(WEBUI.dynamicDomainSet.rulesCountValue, { count: 0 });
  const batchSize =
    typeof config.batch_size === "number" ? config.batch_size : 256;
  const flushInterval =
    typeof config.flush_interval_ms === "number"
      ? config.flush_interval_ms
      : 200;
  return (
    <PluginDetailTemplate
      {...props}
      icon={<List className="h-5 w-5" />}
      summaryItems={[
        { label: t(WEBUI.dynamicDomainSet.ruleFileLabel), value: path },
        {
          label: t(WEBUI.dynamicDomainSet.initialRulesLabel),
          value: bootstrap,
        },
        {
          label: t(WEBUI.dynamicDomainSet.batchThresholdLabel),
          value: String(batchSize),
        },
        {
          label: t(WEBUI.dynamicDomainSet.flushIntervalLabel),
          value: `${flushInterval} ms`,
        },
      ]}
      extraTabs={[
        {
          value: "rules",
          icon: <List className="mr-1 h-3.5 w-3.5" />,
          label: t(WEBUI.dynamicDomainSet.rulesTab),
          content: <RulesPanel tag={props.plugin.name} />,
        },
      ]}
    />
  );
}

function RulesPanel({ tag }: { tag: string }) {
  const appliedStatus = usePluginAppliedStatus(tag);
  if (appliedStatus === "not-applied") {
    return <PluginNotAppliedPlaceholder />;
  }
  return <RulesPanelInner tag={tag} />;
}

function RulesPanelInner({ tag }: { tag: string }) {
  const { t } = useI18n();
  const [rules, setRules] = useState<string[]>([]);
  const [total, setTotal] = useState(0);
  const [nextCursor, setNextCursor] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");

  const [draft, setDraft] = useState("");
  const [draftKind, setDraftKind] = useState<DynamicDomainRuleKind>("full");
  const [adding, setAdding] = useState(false);

  const [removing, setRemoving] = useState<string | null>(null);
  const [clearing, setClearing] = useState(false);
  const [lastNotice, setLastNotice] = useState<string | null>(null);

  // Cancel an in-flight list when a refresh starts so we never apply stale
  // results over a newer set.
  const abortRef = useRef<AbortController | null>(null);

  const load = useCallback(
    async (cursor?: number) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;
      if (cursor === undefined) {
        setLoading(true);
      } else {
        setLoadingMore(true);
      }
      setError(null);
      try {
        const response = await listDynamicDomainRules(tag, {
          cursor,
          limit: PAGE_LIMIT,
          signal: controller.signal,
        });
        if (controller.signal.aborted) return;
        setTotal(response.total);
        setNextCursor(response.next_cursor ?? null);
        setRules((prev) =>
          cursor === undefined ? response.rules : [...prev, ...response.rules],
        );
      } catch (err) {
        if (controller.signal.aborted) return;
        setError(
          err instanceof Error
            ? err.message
            : t(WEBUI.dynamicDomainSet.readRulesFailed),
        );
      } finally {
        if (abortRef.current === controller) {
          abortRef.current = null;
          setLoading(false);
          setLoadingMore(false);
        }
      }
    },
    [tag, t],
  );

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void load();
    }, 0);
    return () => {
      window.clearTimeout(timer);
      abortRef.current?.abort();
    };
  }, [load]);

  const handleAdd = async () => {
    const lines = draft
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.length > 0 && !line.startsWith("#"));
    if (lines.length === 0) {
      setError(t(WEBUI.dynamicDomainSet.inputRequired));
      return;
    }
    setAdding(true);
    setError(null);
    setLastNotice(null);
    try {
      const response = await appendDynamicDomainRules(tag, lines, draftKind);
      setLastNotice(
        t(WEBUI.dynamicDomainSet.addNotice, {
          added: response.added,
          total: response.total,
        }),
      );
      setDraft("");
      await load();
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : t(WEBUI.dynamicDomainSet.addRulesFailed),
      );
    } finally {
      setAdding(false);
    }
  };

  const handleRemove = async (rule: string) => {
    setRemoving(rule);
    setError(null);
    setLastNotice(null);
    try {
      const response = await removeDynamicDomainRules(tag, [rule]);
      setLastNotice(
        t(WEBUI.dynamicDomainSet.removeNotice, {
          removed: response.removed,
          total: response.total,
        }),
      );
      await load();
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : t(WEBUI.dynamicDomainSet.removeRulesFailed),
      );
    } finally {
      setRemoving(null);
    }
  };

  const handleClear = async () => {
    setClearing(true);
    setError(null);
    setLastNotice(null);
    try {
      const response = await clearDynamicDomainRules(tag);
      setLastNotice(
        t(WEBUI.dynamicDomainSet.clearNotice, {
          removed: response.removed,
        }),
      );
      setRules([]);
      setTotal(0);
      setNextCursor(null);
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : t(WEBUI.dynamicDomainSet.clearRulesFailed),
      );
    } finally {
      setClearing(false);
    }
  };

  const trimmedFilter = filter.trim().toLowerCase();
  const visibleRules = trimmedFilter
    ? rules.filter((rule) => rule.toLowerCase().includes(trimmedFilter))
    : rules;

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader className="grid gap-3 p-4 pb-2 sm:grid-cols-[1fr_auto] sm:items-center">
          <div className="min-w-0">
            <CardTitle className="text-sm">
              {t(WEBUI.dynamicDomainSet.addRulesTitle)}
            </CardTitle>
            <p className="mt-1 text-xs text-muted-foreground">
              {t(WEBUI.dynamicDomainSet.addRulesDesc)}
            </p>
          </div>
        </CardHeader>
        <CardContent className="space-y-3 p-4 pt-0">
          <Textarea
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            disabled={adding}
            placeholder={
              "full:login.example.com\ndomain:example.com\nkeyword:cdn"
            }
            className="min-h-[120px] font-mono text-sm"
          />
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <span>{t(WEBUI.dynamicDomainSet.defaultKindLabel)}</span>
              <Select
                value={draftKind}
                onValueChange={(value) =>
                  setDraftKind(value as DynamicDomainRuleKind)
                }
                disabled={adding}
              >
                <SelectTrigger className="h-8 w-32 font-mono">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="full">full</SelectItem>
                  <SelectItem value="domain">domain</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <Button onClick={() => void handleAdd()} disabled={adding}>
              {adding ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Plus className="h-4 w-4" />
              )}
              {t(WEBUI.dynamicDomainSet.addRuleButton)}
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="grid gap-3 p-4 pb-2 sm:grid-cols-[1fr_auto] sm:items-center">
          <div className="min-w-0">
            <CardTitle className="text-sm">
              {t(WEBUI.dynamicDomainSet.currentRulesTitle)}
            </CardTitle>
            <div className="mt-2 flex flex-wrap gap-2 text-xs text-muted-foreground">
              <Badge variant="outline" className="bg-muted/30">
                {t(WEBUI.dynamicDomainSet.totalRules, { count: total })}
              </Badge>
              <Badge variant="outline" className="bg-muted/30">
                {t(WEBUI.dynamicDomainSet.loadedRules, {
                  count: rules.length,
                })}
              </Badge>
              {loading && (
                <Badge
                  variant="outline"
                  className="border-primary/30 bg-primary/10 text-primary"
                >
                  <Loader2 className="mr-1 h-3 w-3 animate-spin" />
                  {t(WEBUI.dynamicDomainSet.loadingRules)}
                </Badge>
              )}
              {lastNotice && !error && (
                <Badge
                  variant="outline"
                  className="border-primary/30 bg-primary/10 text-primary"
                >
                  {lastNotice}
                </Badge>
              )}
            </div>
          </div>
          <div className="flex flex-wrap justify-end gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={loading || clearing}
              onClick={() => void load()}
            >
              <RefreshCw className="h-4 w-4" />
              {t(WEBUI.common.refresh)}
            </Button>
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={clearing || total === 0}
                >
                  {clearing ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Trash2 className="h-4 w-4" />
                  )}
                  {t(WEBUI.dynamicDomainSet.clearRulesButton)}
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogMedia className="bg-destructive/10 text-destructive">
                    <Trash2 className="h-5 w-5" />
                  </AlertDialogMedia>
                  <AlertDialogTitle>
                    {t(WEBUI.dynamicDomainSet.clearRulesTitle)}
                  </AlertDialogTitle>
                  <AlertDialogDescription>
                    {t(WEBUI.dynamicDomainSet.clearRulesDesc, { tag })}
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel disabled={clearing}>
                    {t(WEBUI.common.cancel)}
                  </AlertDialogCancel>
                  <AlertDialogAction
                    variant="destructive"
                    disabled={clearing}
                    onClick={(event) => {
                      event.preventDefault();
                      void handleClear();
                    }}
                  >
                    {t(WEBUI.dynamicDomainSet.clearRulesButton)}
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          </div>
        </CardHeader>
        <CardContent className="space-y-3 p-4 pt-0">
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              placeholder={t(WEBUI.dynamicDomainSet.filterPlaceholder)}
              className="h-8 pl-8 font-mono"
            />
          </div>

          {error && (
            <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              {error}
            </div>
          )}

          <div className="overflow-hidden rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t(WEBUI.dynamicDomainSet.ruleColumn)}</TableHead>
                  <TableHead className="w-24 text-right">
                    {t(WEBUI.dynamicDomainSet.actionColumn)}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {visibleRules.length === 0 && !loading ? (
                  <TableRow>
                    <TableCell
                      colSpan={2}
                      className="text-center text-sm text-muted-foreground"
                    >
                      {trimmedFilter
                        ? t(WEBUI.dynamicDomainSet.noFilteredRules)
                        : t(WEBUI.dynamicDomainSet.noRules)}
                    </TableCell>
                  </TableRow>
                ) : (
                  visibleRules.map((rule) => (
                    <TableRow key={rule}>
                      <TableCell className="font-mono text-sm break-all">
                        {rule}
                      </TableCell>
                      <TableCell className="text-right">
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          disabled={removing === rule}
                          onClick={() => void handleRemove(rule)}
                          aria-label={t(WEBUI.dynamicDomainSet.deleteRuleAria, {
                            rule,
                          })}
                        >
                          {removing === rule ? (
                            <Loader2 className="h-4 w-4 animate-spin" />
                          ) : (
                            <Trash2 className="h-4 w-4 text-destructive" />
                          )}
                        </Button>
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </div>

          {nextCursor !== null && (
            <div className="flex justify-center">
              <Button
                variant="outline"
                size="sm"
                disabled={loadingMore}
                onClick={() => void load(nextCursor)}
              >
                {loadingMore ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : null}
                {t(WEBUI.common.loadMore)}
              </Button>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

export const dynamicDomainSetPlugin: PluginComponentDefinition = {
  Detail: DynamicDomainSetDetail,
};
