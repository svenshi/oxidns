"use client";

import { useState, type ReactNode } from "react";
import { ChevronDown } from "lucide-react";

import { cn } from "@/lib/utils";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

interface AdvancedSettingsSectionProps {
  children: ReactNode;
  defaultOpen?: boolean;
  className?: string;
  contentClassName?: string;
}

export function AdvancedSettingsSection({
  children,
  defaultOpen = false,
  className,
  contentClassName,
}: AdvancedSettingsSectionProps) {
  const { t } = useI18n();
  const [open, setOpen] = useState(defaultOpen);

  return (
    <div className={cn("w-full rounded-lg border border-border/70", className)}>
      <button
        type="button"
        className="flex w-full items-center justify-between px-3 py-2 text-sm font-medium"
        onClick={() => setOpen((current) => !current)}
        aria-expanded={open}
      >
        {t(WEBUI.plugins.advancedSettings)}
        <ChevronDown
          className={cn("size-4 transition-transform", open && "rotate-180")}
        />
      </button>
      {open && (
        <div className={cn("border-t p-3", contentClassName)}>{children}</div>
      )}
    </div>
  );
}
