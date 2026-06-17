"use client";

import { useEffect, useMemo, useState } from "react";
import { RefreshCw, Search } from "lucide-react";
import { AppHeader } from "@/components/shell/app-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { fetchQueryRecords, type QueryRecordRow } from "@/lib/oxidns-api";
import { selectStandardCapabilityMap } from "@/lib/standard-mode/selectors";
import { useAppStore } from "@/lib/store";

function question(record: QueryRecordRow) {
  return (
    record.questions_json
      ?.map((item) => `${item.name} ${item.qtype}`)
      .join(", ") || "-"
  );
}

export default function StandardQueriesPage() {
  const plugins = useAppStore((s) => s.plugins);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const capabilities = useMemo(
    () => selectStandardCapabilityMap(buildInfo),
    [buildInfo],
  );
  const recorder =
    plugins.find((plugin) => plugin.name === "standard_recorder") ??
    plugins.find((plugin) => plugin.pluginKind === "query_recorder");
  const recorderName = recorder?.name;
  const [records, setRecords] = useState<QueryRecordRow[]>([]);
  const [qname, setQname] = useState("");
  const [clientIp, setClientIp] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    if (!recorderName) return;
    setLoading(true);
    setError(null);
    fetchQueryRecords(recorderName, {
      limit: 80,
      qname: qname.trim() || undefined,
      clientIp: clientIp.trim() || undefined,
    })
      .then((response) => setRecords(response.records))
      .catch((err) =>
        setError(err instanceof Error ? err.message : "查询日志读取失败"),
      )
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    if (!recorderName) return;
    const controller = new AbortController();
    fetchQueryRecords(recorderName, { limit: 80, signal: controller.signal })
      .then((response) => setRecords(response.records))
      .catch((err) => {
        if (!controller.signal.aborted) {
          setError(err instanceof Error ? err.message : "查询日志读取失败");
        }
      });
    return () => controller.abort();
  }, [recorderName]);

  return (
    <>
      <AppHeader title="查询日志" />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="space-y-6">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h1 className="text-xl font-semibold tracking-tight">
                DNS 查询日志
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                复用 query_recorder 插件接口展示最近查询、错误和响应耗时。
              </p>
            </div>
            <Button onClick={load} disabled={!recorderName || loading}>
              <RefreshCw className="size-4" />
              刷新
            </Button>
          </div>

          {!capabilities.queryRecorder || !recorderName ? (
            <Card>
              <CardContent className="p-6 text-sm text-muted-foreground">
                当前构建或当前配置未启用
                query_recorder。启用标准模式查询日志后，这里会显示查询列表和筛选结果。
              </CardContent>
            </Card>
          ) : (
            <Card>
              <CardHeader className="flex flex-row items-center justify-between space-y-0">
                <CardTitle className="text-base">记录列表</CardTitle>
                <Badge variant="secondary">{recorderName}</Badge>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="grid gap-3 md:grid-cols-[1fr_220px_auto]">
                  <div className="relative">
                    <Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
                    <Input
                      className="pl-8"
                      value={qname}
                      onChange={(event) => setQname(event.target.value)}
                      placeholder="域名过滤"
                    />
                  </div>
                  <Input
                    value={clientIp}
                    onChange={(event) => setClientIp(event.target.value)}
                    placeholder="客户端 IP"
                  />
                  <Button variant="secondary" onClick={load} disabled={loading}>
                    查询
                  </Button>
                </div>
                {error ? (
                  <div className="rounded-md border border-destructive/40 p-3 text-sm text-destructive">
                    {error}
                  </div>
                ) : null}
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>时间</TableHead>
                      <TableHead>客户端</TableHead>
                      <TableHead>问题</TableHead>
                      <TableHead>RCODE</TableHead>
                      <TableHead className="text-right">耗时</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {records.map((record) => (
                      <TableRow key={record.id}>
                        <TableCell className="whitespace-nowrap text-muted-foreground">
                          {new Date(record.created_at_ms).toLocaleString()}
                        </TableCell>
                        <TableCell>{record.client_ip}</TableCell>
                        <TableCell className="max-w-[420px] truncate">
                          {question(record)}
                        </TableCell>
                        <TableCell>
                          {record.rcode ?? (record.error ? "ERROR" : "-")}
                        </TableCell>
                        <TableCell className="text-right">
                          {record.elapsed_ms} ms
                        </TableCell>
                      </TableRow>
                    ))}
                    {records.length === 0 ? (
                      <TableRow>
                        <TableCell
                          colSpan={5}
                          className="h-24 text-center text-muted-foreground"
                        >
                          暂无记录。
                        </TableCell>
                      </TableRow>
                    ) : null}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>
          )}
        </div>
      </main>
    </>
  );
}
