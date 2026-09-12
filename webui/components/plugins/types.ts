import type { ComponentType } from "react";
import type { ReactNode } from "react";
import type { PluginInstance } from "@/lib/types";

export interface PluginCardComponentProps {
  plugin: PluginInstance;
  compact?: boolean;
}

export interface PluginMetricPoint {
  time: string;
  qps: number;
  latency: number;
}

export interface PluginDetailComponentProps {
  plugin: PluginInstance;
  chartData: PluginMetricPoint[];
  onClose: () => void;
}

export interface PluginSummaryItem {
  label: string;
  value: string;
}

export interface PluginCardTemplateProps extends PluginCardComponentProps {
  icon?: ReactNode;
  primaryMetric?: {
    label: string;
    value: string;
  };
  children?: ReactNode;
}

export interface PluginExtraTab {
  /** Tabs primitive value; must be unique within the detail view. */
  value: string;
  /** Label rendered inside the TabsTrigger; PluginDetailTemplate keeps the icon outside. */
  label: ReactNode;
  /** Optional icon rendered before the label. */
  icon?: ReactNode;
  /** Tab body. */
  content: ReactNode;
}

export interface PluginDetailTemplateProps extends PluginDetailComponentProps {
  icon?: ReactNode;
  summaryItems?: PluginSummaryItem[];
  configContent?: ReactNode;
  metricsContent?: ReactNode;
  runtimeContent?: ReactNode;
  /**
   * Extra top-level tabs rendered after the metrics tab and before built-in metrics.
   * Use this when a plugin wants to expose a view that is conceptually peer to
   * "config" / "stats" rather than nested under one of them.
   */
  extraTabs?: PluginExtraTab[];
}

export interface PluginComponentDefinition {
  Card?: ComponentType<PluginCardComponentProps>;
  Detail?: ComponentType<PluginDetailComponentProps>;
}
