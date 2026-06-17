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
      <AppHeader title="广告过滤" />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-5xl space-y-6">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h1 className="text-xl font-semibold tracking-tight">
                广告过滤
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                手动黑名单和白名单会保存到标准模式 schema v2。
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
              {!capabilities.adRules || !capabilities.blackHole ? (
                <Badge variant="secondary">当前构建不支持过滤闭环</Badge>
              ) : null}
            </CardHeader>
            <CardContent className="space-y-5">
              <label className="flex items-center justify-between rounded-md border p-3 text-sm">
                <span>启用广告拦截</span>
                <Switch
                  checked={settings.filtering.enabled}
                  disabled={!capabilities.adRules || !capabilities.blackHole}
                  onCheckedChange={(checked) =>
                    setPartial({
                      filtering: { ...settings.filtering, enabled: checked },
                    })
                  }
                />
              </label>
              <label className="space-y-2 block">
                <span className="text-sm font-medium">黑名单规则</span>
                <Textarea
                  value={settings.filtering.blockRules.join("\n")}
                  disabled={
                    !settings.filtering.enabled ||
                    !capabilities.adRules ||
                    !capabilities.blackHole
                  }
                  onChange={(event) =>
                    setPartial({
                      filtering: {
                        ...settings.filtering,
                        blockRules: lines(event.target.value),
                      },
                    })
                  }
                  rows={5}
                  placeholder={"||example.com^\n||ads.example.net^"}
                />
              </label>
              <label className="space-y-2 block">
                <span className="text-sm font-medium">白名单规则</span>
                <Textarea
                  value={settings.filtering.allowRules.join("\n")}
                  disabled={!settings.filtering.enabled}
                  onChange={(event) =>
                    setPartial({
                      filtering: {
                        ...settings.filtering,
                        allowRules: lines(event.target.value),
                      },
                    })
                  }
                  rows={5}
                  placeholder={"@@||allowed.example.com^"}
                />
              </label>
            </CardContent>
          </Card>
        </div>
      </main>
    </>
  );
}
