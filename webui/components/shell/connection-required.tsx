/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import Link from "next/link";
import {
  PlugZap,
  FileCode2,
  Loader2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/lib/store";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

export { LoginView as LoginRequired } from "@/components/auth/login-view";

export function ConnectionPending() {
  const { t } = useI18n();
  return (
    <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
      <Card className="max-w-xl">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Loader2 className="h-5 w-5 animate-spin" />
            {t(WEBUI.connection.pendingTitle)}
          </CardTitle>
          <CardDescription>{t(WEBUI.connection.pendingDesc)}</CardDescription>
        </CardHeader>
      </Card>
    </main>
  );
}

export function ConnectionRequired() {
  const { t } = useI18n();
  const setEditorMode = useAppStore((s) => s.setEditorMode);
  return (
    <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
      <Card className="max-w-xl">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <PlugZap className="h-5 w-5" />
            {t(WEBUI.connection.requiredTitle)}
          </CardTitle>
          <CardDescription>{t(WEBUI.connection.requiredDesc)}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          <Button asChild>
            <Link href="/settings">{t(WEBUI.connection.goSettings)}</Link>
          </Button>
          <Button variant="outline" onClick={() => setEditorMode(true)}>
            <FileCode2 className="h-4 w-4 mr-1.5" />
            {t(WEBUI.connection.offlineEditConfigFile)}
          </Button>
        </CardContent>
      </Card>
    </main>
  );
}
