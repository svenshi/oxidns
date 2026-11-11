/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { useEffect } from "react";
import { AppHeader } from "@/components/shell/app-header";
import { LogViewer } from "@/components/logs/log-viewer";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

export default function LogsPage() {
  const { t } = useI18n();
  useEffect(() => {
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = prev;
    };
  }, []);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <AppHeader title={t(WEBUI.shell.logs)} />
      <div className="flex min-h-0 flex-1">
        <LogViewer />
      </div>
    </div>
  );
}
