"use client";

import { GitBranch } from "lucide-react";
import { StandardPlaceholderPage } from "@/components/standard/standard-placeholder-page";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store";

export default function StandardRoutingPage() {
  const { t } = useI18n();
  const routing = useAppStore((s) => s.standardSettings.routing);

  return (
    <StandardPlaceholderPage
      titleKey={WEBUI.standardNav.routing}
      descriptionKey={WEBUI.standardShell.routingDescription}
      emptyTitleKey={WEBUI.standardShell.routingEmptyTitle}
      emptyDescriptionKey={WEBUI.standardShell.routingEmptyDescription}
      badgeKey={WEBUI.standardShell.phase3SkeletonBadge}
      icon={GitBranch}
      metrics={[
        {
          label: t(WEBUI.standardShell.routingRulesMetric),
          value: routing.rules.length,
        },
        {
          label: t(WEBUI.standardShell.routingScenariosMetric),
          value: routing.scenarios.length,
        },
        {
          label: t(WEBUI.standardShell.enabledRulesMetric),
          value: routing.rules.filter((rule) => rule.enabled).length,
        },
      ]}
    />
  );
}
