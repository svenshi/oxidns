"use client";

import { useMemo, useState } from "react";
import { Save, ShieldCheck } from "lucide-react";
import { AppHeader } from "@/components/shell/app-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { selectStandardCapabilityMap } from "@/lib/standard-mode/selectors";
import type { StandardModeSettings } from "@/lib/standard-mode/types";
import { useAppStore } from "@/lib/store";

function lines(value: string) {
  return value
    .split("\n")
    .map((item) => item.trim())
    .filter(Boolean);
}

export default function StandardFilteringPage() {
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
      <AppHeader title="过滤与分流" />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-5xl space-y-6">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h1 className="text-xl font-semibold tracking-tight">
                广告拦截与国内外分流
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                内联规则和分流域名会转换成标准模式的 provider 与 sequence 配置。
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
            <CardHeader className="flex flex-row items-center justify-between space-y-0">
              <CardTitle className="flex items-center gap-2 text-base">
                <ShieldCheck className="size-4" />
                广告拦截
              </CardTitle>
              {!capabilities.adRules ? (
                <Badge variant="secondary">当前构建不支持 adguard_rule</Badge>
              ) : null}
            </CardHeader>
            <CardContent className="space-y-5">
              <label className="flex items-center justify-between rounded-md border p-3 text-sm">
                <span>启用广告拦截</span>
                <Switch
                  checked={settings.adBlock.enabled}
                  disabled={!capabilities.adRules}
                  onCheckedChange={(checked) =>
                    setPartial({
                      adBlock: { ...settings.adBlock, enabled: checked },
                    })
                  }
                />
              </label>
              <label className="space-y-2 block">
                <span className="text-sm font-medium">内联规则</span>
                <Textarea
                  value={settings.adBlock.inlineRules.join("\n")}
                  disabled={!settings.adBlock.enabled || !capabilities.adRules}
                  onChange={(event) =>
                    setPartial({
                      adBlock: {
                        ...settings.adBlock,
                        inlineRules: lines(event.target.value),
                      },
                    })
                  }
                  rows={5}
                  placeholder={"||example.com^\n@@||allowed.example.com^"}
                />
              </label>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex flex-row items-center justify-between space-y-0">
              <CardTitle className="text-base">国内外分流</CardTitle>
              {!capabilities.domainSet ? (
                <Badge variant="secondary">当前构建不支持 domain_set</Badge>
              ) : null}
            </CardHeader>
            <CardContent className="space-y-5">
              <label className="flex items-center justify-between rounded-md border p-3 text-sm">
                <span>启用分流</span>
                <Switch
                  checked={settings.split.enabled}
                  disabled={!capabilities.domainSet || !capabilities.forward}
                  onCheckedChange={(checked) =>
                    setPartial({
                      split: { ...settings.split, enabled: checked },
                    })
                  }
                />
              </label>
              <label className="space-y-2 block">
                <span className="text-sm font-medium">国内域名规则</span>
                <Textarea
                  value={settings.split.domesticDomains.join("\n")}
                  disabled={!settings.split.enabled}
                  onChange={(event) =>
                    setPartial({
                      split: {
                        ...settings.split,
                        domesticDomains: lines(event.target.value),
                      },
                    })
                  }
                  rows={5}
                  placeholder={"geosite:cn\nexample.cn"}
                />
              </label>
              <label className="space-y-2 block">
                <span className="text-sm font-medium">国内上游</span>
                <Textarea
                  value={settings.split.domesticUpstreams
                    .map((item) => item.address)
                    .join("\n")}
                  disabled={!settings.split.enabled}
                  onChange={(event) =>
                    setPartial({
                      split: {
                        ...settings.split,
                        domesticUpstreams: lines(event.target.value).map(
                          (address, index) => ({
                            id: `domestic_${index + 1}`,
                            name: address,
                            address,
                            enabled: true,
                            group: "domestic",
                          }),
                        ),
                      },
                    })
                  }
                  rows={4}
                  placeholder={"223.5.5.5:53\n119.29.29.29:53"}
                />
              </label>
            </CardContent>
          </Card>
        </div>
      </main>
    </>
  );
}
