"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import {
  List,
  Loader2,
  Plus,
  RefreshCw,
  Search,
  Trash2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
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

const PAGE_LIMIT = 200;

function DynamicDomainSetDetail(props: PluginDetailComponentProps) {
  const config = props.plugin.config as Record<string, unknown>;
  const path = typeof config.path === "string" ? config.path : "-";
  const bootstrap = Array.isArray(config.bootstrap_rules)
    ? `${config.bootstrap_rules.length} 条`
    : "0 条";
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
        { label: "规则文件", value: path },
        { label: "初始规则", value: bootstrap },
        { label: "批量阈值", value: String(batchSize) },
        { label: "Flush 间隔", value: `${flushInterval} ms` },
      ]}
      extraTabs={[
        {
          value: "rules",
          icon: <List className="mr-1 h-3.5 w-3.5" />,
          label: "规则",
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
        setError(err instanceof Error ? err.message : "读取规则失败");
      } finally {
        if (abortRef.current === controller) {
          abortRef.current = null;
          setLoading(false);
          setLoadingMore(false);
        }
      }
    },
    [tag],
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
      setError("请至少输入一条规则");
      return;
    }
    setAdding(true);
    setError(null);
    setLastNotice(null);
    try {
      const response = await appendDynamicDomainRules(tag, lines, draftKind);
      setLastNotice(
        `新增 ${response.added} 条，当前共 ${response.total} 条`,
      );
      setDraft("");
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "添加规则失败");
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
        `删除 ${response.removed} 条，当前共 ${response.total} 条`,
      );
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "删除规则失败");
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
      setLastNotice(`已清空 ${response.removed} 条`);
      setRules([]);
      setTotal(0);
      setNextCursor(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "清空规则失败");
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
            <CardTitle className="text-sm">添加规则</CardTitle>
            <p className="mt-1 text-xs text-muted-foreground">
              每行一条规则，支持 <code>full:</code>、<code>domain:</code>、
              <code>keyword:</code>、<code>regexp:</code> 与无前缀域名；空行与
              <code> # </code>开头的注释会被忽略。
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
              <span>无前缀域名按以下类型解析：</span>
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
              添加
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="grid gap-3 p-4 pb-2 sm:grid-cols-[1fr_auto] sm:items-center">
          <div className="min-w-0">
            <CardTitle className="text-sm">当前规则</CardTitle>
            <div className="mt-2 flex flex-wrap gap-2 text-xs text-muted-foreground">
              <Badge variant="outline" className="bg-muted/30">
                共 {total} 条
              </Badge>
              <Badge variant="outline" className="bg-muted/30">
                已载入 {rules.length} 条
              </Badge>
              {loading && (
                <Badge
                  variant="outline"
                  className="border-primary/30 bg-primary/10 text-primary"
                >
                  <Loader2 className="mr-1 h-3 w-3 animate-spin" />
                  正在加载
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
              刷新
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
                  清空规则
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogMedia className="bg-destructive/10 text-destructive">
                    <Trash2 className="h-5 w-5" />
                  </AlertDialogMedia>
                  <AlertDialogTitle>清空所有规则？</AlertDialogTitle>
                  <AlertDialogDescription>
                    将删除 &ldquo;{tag}&rdquo;
                    管理文件中的所有规则，并替换为空快照。此操作无法撤销。
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel disabled={clearing}>
                    取消
                  </AlertDialogCancel>
                  <AlertDialogAction
                    variant="destructive"
                    disabled={clearing}
                    onClick={(event) => {
                      event.preventDefault();
                      void handleClear();
                    }}
                  >
                    清空规则
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
              placeholder="按内容过滤已加载的规则"
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
                  <TableHead>规则</TableHead>
                  <TableHead className="w-24 text-right">操作</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {visibleRules.length === 0 && !loading ? (
                  <TableRow>
                    <TableCell
                      colSpan={2}
                      className="text-center text-sm text-muted-foreground"
                    >
                      {trimmedFilter ? "没有匹配过滤条件的规则" : "暂无规则"}
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
                          aria-label={`删除规则 ${rule}`}
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
                加载更多
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
