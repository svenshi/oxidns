"use client";

import { ShieldAlert } from "lucide-react";
import { StandardPlaceholderPage } from "@/components/standard/standard-placeholder-page";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store";

export default function StandardExceptionsPage() {
  const { t } = useI18n();
  const exceptions = useAppStore((s) => s.standardSettings.exceptions);

  return (
    <StandardPlaceholderPage
      titleKey={WEBUI.standardNav.exceptions}
      descriptionKey={WEBUI.standardShell.exceptionsDescription}
      emptyTitleKey={WEBUI.standardShell.exceptionsEmptyTitle}
      emptyDescriptionKey={WEBUI.standardShell.exceptionsEmptyDescription}
      badgeKey={WEBUI.standardShell.phase3SkeletonBadge}
      icon={ShieldAlert}
      metrics={[
        {
          label: t(WEBUI.standardShell.exceptionsMetric),
          value: exceptions.length,
        },
        {
          label: t(WEBUI.standardShell.enabledRulesMetric),
          value: exceptions.filter((rule) => rule.enabled).length,
        },
      ]}
    />
  );
}
