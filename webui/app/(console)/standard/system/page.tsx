"use client";

import { useState } from "react";
import { Loader2, Save } from "lucide-react";
import { AppHeader } from "@/components/shell/app-header";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import type { StandardModeSettings } from "@/lib/standard-mode/types";
import { useAppStore } from "@/lib/store";

const logLevels: StandardModeSettings["system"]["logLevel"][] = [
  "trace",
  "debug",
  "info",
  "warn",
  "error",
];

export default function StandardSystemPage() {
  const { t } = useI18n();
  const stored = useAppStore((state) => state.standardSettings);
  const saveStandardSettings = useAppStore(
    (state) => state.saveStandardSettings,
  );
  const busy = useAppStore((state) => state.isConfigSaving || state.isApplying);
  const [draft, setDraft] = useState<StandardModeSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const settings = draft ?? stored;

  const updateSystem = (patch: Partial<StandardModeSettings["system"]>) => {
    setError(null);
    setDraft({
      ...settings,
      system: { ...settings.system, ...patch },
    });
  };

  const save = async () => {
    setError(null);
    try {
      await saveStandardSettings(settings, { apply: true });
      setDraft(settings);
    } catch (saveError) {
      setError(
        saveError instanceof Error ? saveError.message : String(saveError),
      );
    }
  };

  return (
    <>
      <AppHeader title={t(WEBUI.standardSystem.title)} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-3xl space-y-5">
          <div className="flex items-start justify-between gap-4">
            <p className="text-sm leading-6 text-muted-foreground">
              {t(WEBUI.standardSystem.description)}
            </p>
            <Button disabled={busy} onClick={save}>
              {busy ? <Loader2 className="size-4 animate-spin" /> : <Save />}
              {busy
                ? t(WEBUI.standardSystem.savingApplying)
                : t(WEBUI.standardSystem.saveApply)}
            </Button>
          </div>

          {error ? (
            <p className="whitespace-pre-line rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
              {error}
            </p>
          ) : null}

          <Card>
            <CardHeader>
              <CardTitle className="text-base">
                {t(WEBUI.standardSystem.runtimeTitle)}
              </CardTitle>
            </CardHeader>
            <CardContent className="grid gap-5 sm:grid-cols-2">
              <div className="space-y-2">
                <Label>{t(WEBUI.standardSystem.logLevel)}</Label>
                <Select
                  value={settings.system.logLevel}
                  onValueChange={(value) =>
                    updateSystem({
                      logLevel:
                        value as StandardModeSettings["system"]["logLevel"],
                    })
                  }
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {logLevels.map((level) => (
                      <SelectItem key={level} value={level}>
                        {level}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <div className="space-y-2">
                <Label htmlFor="standard-worker-threads">
                  {t(WEBUI.standardSystem.workerThreads)}
                </Label>
                <Input
                  id="standard-worker-threads"
                  type="number"
                  min={1}
                  placeholder={t(WEBUI.standardSystem.workerThreadsAuto)}
                  value={settings.system.threads ?? ""}
                  onChange={(event) => {
                    const value = event.target.value;
                    updateSystem({
                      threads: value
                        ? Math.max(1, Math.trunc(Number(value) || 1))
                        : undefined,
                    });
                  }}
                />
                <p className="text-xs text-muted-foreground">
                  {t(WEBUI.standardSystem.workerThreadsHint)}
                </p>
              </div>
            </CardContent>
          </Card>
        </div>
      </main>
    </>
  );
}
