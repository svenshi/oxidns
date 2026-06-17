"use client";

import { Smartphone } from "lucide-react";
import { StandardPlaceholderPage } from "@/components/standard/standard-placeholder-page";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store";

export default function StandardDevicesPage() {
  const { t } = useI18n();
  const devices = useAppStore((s) => s.standardSettings.devices);
  const assignedDevices = devices.filter((device) => device.assignedPathId);

  return (
    <StandardPlaceholderPage
      titleKey={WEBUI.standardNav.devices}
      descriptionKey={WEBUI.standardShell.devicesDescription}
      emptyTitleKey={WEBUI.standardShell.devicesEmptyTitle}
      emptyDescriptionKey={WEBUI.standardShell.devicesEmptyDescription}
      badgeKey={WEBUI.standardShell.phase3SkeletonBadge}
      icon={Smartphone}
      metrics={[
        {
          label: t(WEBUI.standardShell.devicesMetric),
          value: devices.length,
        },
        {
          label: t(WEBUI.standardShell.assignedDevicesMetric),
          value: assignedDevices.length,
        },
      ]}
    />
  );
}
