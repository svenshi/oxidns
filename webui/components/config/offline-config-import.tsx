/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { useRef, useState } from "react";
import { FileUp, ClipboardPaste, FileWarning, LogOut } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Textarea } from "@/components/ui/textarea";
import { useAppStore } from "@/lib/store";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

export function OfflineConfigImport() {
  const { t } = useI18n();
  const enterOfflineConfig = useAppStore((s) => s.enterOfflineConfig);
  const setEditorMode = useAppStore((s) => s.setEditorMode);
  const [text, setText] = useState("");
  const [fileName, setFileName] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleFile = async (file: File) => {
    const content = await file.text();
    setText(content);
    setFileName(file.name);
  };

  const handleStart = () => {
    if (!text.trim()) return;
    enterOfflineConfig(text, fileName ?? undefined);
  };

  return (
    <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
      <Card className="max-w-2xl">
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-lg">
            <ClipboardPaste className="h-5 w-5" />
            {t(WEBUI.configEditor.offlineImportTitle)}
          </CardTitle>
          <CardDescription>
            {t(WEBUI.configEditor.offlineImportDesc)}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <Textarea
            value={text}
            onChange={(event) => {
              setText(event.target.value);
              setFileName(null);
            }}
            placeholder={t(WEBUI.configEditor.pastePlaceholder)}
            className="h-64 font-mono text-sm"
            spellCheck={false}
          />
          {fileName && (
            <p className="text-xs text-muted-foreground">
              {t(WEBUI.configEditor.loadedFile, { file: fileName })}
            </p>
          )}
          <div className="flex flex-wrap items-center gap-2">
            <input
              ref={fileInputRef}
              type="file"
              accept=".yaml,.yml,text/yaml,application/x-yaml"
              className="hidden"
              onChange={(event) => {
                const file = event.target.files?.[0];
                if (file) void handleFile(file);
                event.target.value = "";
              }}
            />
            <Button
              variant="outline"
              onClick={() => fileInputRef.current?.click()}
            >
              <FileUp className="h-4 w-4 mr-1.5" />
              {t(WEBUI.configEditor.uploadFile)}
            </Button>
            <Button onClick={handleStart} disabled={!text.trim()}>
              <ClipboardPaste className="h-4 w-4 mr-1.5" />
              {t(WEBUI.configEditor.startOfflineEdit)}
            </Button>
            <Button
              variant="ghost"
              className="ml-auto"
              onClick={() => setEditorMode(false)}
            >
              <LogOut className="h-4 w-4 mr-1.5" />
              {t(WEBUI.configEditor.exit)}
            </Button>
          </div>
          <div className="flex items-start gap-2 rounded-lg border border-yellow-500/30 bg-yellow-500/10 px-3 py-2 text-xs text-yellow-700 dark:text-yellow-400">
            <FileWarning className="h-4 w-4 shrink-0" />
            <span>{t(WEBUI.configEditor.offlineWarning)}</span>
          </div>
        </CardContent>
      </Card>
    </main>
  );
}
