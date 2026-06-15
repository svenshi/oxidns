"use client";

import { useMemo, useState } from "react";
import { Save } from "lucide-react";
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
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { selectStandardCapabilityMap } from "@/lib/standard-mode/selectors";
import type {
  StandardModeSettings,
  StandardUpstream,
} from "@/lib/standard-mode/types";
import { useAppStore } from "@/lib/store";

function upstreamText(upstreams: StandardUpstream[]) {
  return upstreams
    .filter((item) => item.enabled)
    .map((item) => item.address)
    .join("\n");
}

function parseUpstreams(
  value: string,
  group: "global" | "domestic",
): StandardUpstream[] {
  return value
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((address, index) => ({
      id: `${group}_${index + 1}`,
      name: address,
      address,
      enabled: true,
      group,
    }));
}

export default function StandardDnsPage() {
  const storeSettings = useAppStore((s) => s.standardSettings);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const saveStandardSettings = useAppStore((s) => s.saveStandardSettings);
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const capabilities = useMemo(
    () => selectStandardCapabilityMap(buildInfo),
    [buildInfo],
  );
  const [draftSettings, setDraftSettings] =
    useState<StandardModeSettings | null>(null);
  const settings = draftSettings ?? storeSettings;

  const setPartial = (patch: Partial<StandardModeSettings>) => {
    setDraftSettings((current) => ({ ...(current ?? settings), ...patch }));
  };

  return (
    <>
      <AppHeader
        title="DNS 设置"
        breadcrumbs={[{ label: "标准模式", href: "/standard" }]}
      />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-5xl space-y-6">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h1 className="text-xl font-semibold tracking-tight">
                基础 DNS 设置
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                保存后会根据当前表单生成标准模式负责的 YAML 插件配置。
              </p>
            </div>
            <Button
              onClick={() => saveStandardSettings(settings)}
              disabled={isConfigSaving}
            >
              <Save className="size-4" />
              保存标准配置
            </Button>
          </div>

          <Card>
            <CardHeader>
              <CardTitle className="text-base">监听与上游</CardTitle>
            </CardHeader>
            <CardContent className="grid gap-5 md:grid-cols-2">
              <label className="space-y-2">
                <span className="text-sm font-medium">监听地址</span>
                <Input
                  value={settings.listen.address}
                  onChange={(event) =>
                    setPartial({
                      listen: {
                        ...settings.listen,
                        address: event.target.value,
                      },
                    })
                  }
                  placeholder="0.0.0.0:5335"
                />
              </label>
              <div className="flex items-center gap-6 pt-7">
                <label className="flex items-center gap-2 text-sm">
                  <Switch
                    checked={settings.listen.udp}
                    onCheckedChange={(checked) =>
                      setPartial({
                        listen: { ...settings.listen, udp: checked },
                      })
                    }
                  />
                  UDP
                </label>
                <label className="flex items-center gap-2 text-sm">
                  <Switch
                    checked={settings.listen.tcp}
                    onCheckedChange={(checked) =>
                      setPartial({
                        listen: { ...settings.listen, tcp: checked },
                      })
                    }
                  />
                  TCP
                </label>
              </div>
              <label className="space-y-2 md:col-span-2">
                <span className="text-sm font-medium">全局上游</span>
                <Textarea
                  value={upstreamText(settings.upstreams)}
                  onChange={(event) =>
                    setPartial({
                      upstreams: parseUpstreams(event.target.value, "global"),
                    })
                  }
                  rows={5}
                  placeholder={"1.1.1.1:53\n8.8.8.8:53"}
                />
              </label>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex flex-row items-center justify-between space-y-0">
              <CardTitle className="text-base">缓存、双栈与 IP 优选</CardTitle>
              {!capabilities.cache ? (
                <Badge variant="secondary">当前构建不支持 cache</Badge>
              ) : null}
            </CardHeader>
            <CardContent className="grid gap-5 md:grid-cols-2">
              <label className="flex items-center justify-between rounded-md border p-3 text-sm">
                <span>启用缓存</span>
                <Switch
                  checked={settings.cache.enabled}
                  disabled={!capabilities.cache}
                  onCheckedChange={(checked) =>
                    setPartial({
                      cache: { ...settings.cache, enabled: checked },
                    })
                  }
                />
              </label>
              <label className="space-y-2">
                <span className="text-sm font-medium">缓存容量</span>
                <Input
                  type="number"
                  min={128}
                  value={settings.cache.size}
                  disabled={!settings.cache.enabled || !capabilities.cache}
                  onChange={(event) =>
                    setPartial({
                      cache: {
                        ...settings.cache,
                        size: Number(event.target.value) || 8192,
                      },
                    })
                  }
                />
              </label>
              <label className="space-y-2">
                <span className="text-sm font-medium">双栈策略</span>
                <Select
                  value={settings.dualStack.strategy}
                  onValueChange={(value) =>
                    setPartial({
                      dualStack: {
                        strategy:
                          value as StandardModeSettings["dualStack"]["strategy"],
                      },
                    })
                  }
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="auto">自动</SelectItem>
                    <SelectItem value="prefer_ipv4">优先 IPv4</SelectItem>
                    <SelectItem value="prefer_ipv6">优先 IPv6</SelectItem>
                    <SelectItem value="ipv4_only">仅 IPv4</SelectItem>
                    <SelectItem value="ipv6_only">仅 IPv6</SelectItem>
                  </SelectContent>
                </Select>
              </label>
              <label className="flex items-center justify-between rounded-md border p-3 text-sm">
                <span>启用 IP 优选</span>
                <Switch
                  checked={settings.ipSelection.enabled}
                  disabled={!capabilities.ipSelector}
                  onCheckedChange={(checked) =>
                    setPartial({
                      ipSelection: {
                        ...settings.ipSelection,
                        enabled: checked,
                      },
                    })
                  }
                />
              </label>
              <label className="flex items-center justify-between rounded-md border p-3 text-sm">
                <span>启用 ECS</span>
                <Switch
                  checked={settings.ecs.enabled}
                  onCheckedChange={(checked) =>
                    setPartial({ ecs: { ...settings.ecs, enabled: checked } })
                  }
                />
              </label>
            </CardContent>
          </Card>
        </div>
      </main>
    </>
  );
}
