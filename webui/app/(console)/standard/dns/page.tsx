"use client";

import { useMemo, useState } from "react";
import { Save } from "lucide-react";
import { AppHeader } from "@/components/shell/app-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import {
  selectDefaultUpstreamGroup,
  selectStandardCapabilityMap,
} from "@/lib/standard-mode/selectors";
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

function parseUpstreams(value: string): StandardUpstream[] {
  return value
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((address, index) => ({
      id: `default_${index + 1}`,
      name: address,
      protocol: "auto",
      address,
      enabled: true,
      tlsVerify: true,
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
  const defaultGroup = selectDefaultUpstreamGroup(settings);

  const setPartial = (patch: Partial<StandardModeSettings>) => {
    setDraftSettings((current) => ({ ...(current ?? settings), ...patch }));
  };
  const setDefaultUpstreams = (upstreams: StandardUpstream[]) => {
    const defaultGroupId = defaultGroup.id;
    setPartial({
      upstreamGroups: settings.upstreamGroups.map((group, index) =>
        group.id === defaultGroupId || (index === 0 && defaultGroupId === group.id)
          ? { ...group, upstreams }
          : group,
      ),
    });
  };

  return (
    <>
      <AppHeader title="DNS 设置" />
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
                  value={upstreamText(defaultGroup.upstreams)}
                  onChange={(event) =>
                    setDefaultUpstreams(parseUpstreams(event.target.value))
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
              <label className="flex items-center justify-between rounded-md border p-3 text-sm">
                <span>启用查询日志</span>
                <Switch
                  checked={settings.queryLog.enabled}
                  disabled={!capabilities.queryRecorder}
                  onCheckedChange={(checked) =>
                    setPartial({
                      queryLog: {
                        ...settings.queryLog,
                        enabled: checked,
                      },
                    })
                  }
                />
              </label>
              <label className="space-y-2">
                <span className="text-sm font-medium">日志保留天数</span>
                <Input
                  type="number"
                  min={1}
                  value={settings.queryLog.retentionDays}
                  disabled={!settings.queryLog.enabled || !capabilities.queryRecorder}
                  onChange={(event) =>
                    setPartial({
                      queryLog: {
                        ...settings.queryLog,
                        retentionDays: Number(event.target.value) || 7,
                      },
                    })
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
