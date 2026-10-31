"use client";

import { DnsObservability } from "@/components/observability/dns-observability";
import { AppHeader } from "@/components/shell/app-header";
import { useI18n } from "@/lib/i18n/provider";
import { WEBUI } from "@/lib/i18n";

export default function ObservabilityPage() {
  const { t } = useI18n();
  return (
    <>
      <AppHeader title={t(WEBUI.shell.observability)} />
      <DnsObservability />
    </>
  );
}
