"use client";

import { useEffect, useMemo, useState } from "react";
import { AppHeader } from "@/components/shell/app-header";
import { SystemMetrics } from "@/components/dashboard/system-metrics";
import { SortablePluginGrid } from "@/components/plugins/sortable-plugin-grid";
import { useAppStore } from "@/lib/store";
import type { PluginInstance } from "@/lib/types";
import Link from "next/link";
import { Button } from "@/components/ui/button";
import { ArrowRight } from "lucide-react";

// Dashboard card order is a frontend-only preference: it lives in
// localStorage and never touches the config file (unlike the plugin center,
// where reordering rewrites the YAML order).
const DASHBOARD_ORDER_KEY = "oxidns:dashboard-order";

function loadDashboardOrder(): string[] {
  try {
    const stored = localStorage.getItem(DASHBOARD_ORDER_KEY);
    return stored ? (JSON.parse(stored) as string[]) : [];
  } catch {
    return [];
  }
}

function saveDashboardOrder(ids: string[]): void {
  try {
    localStorage.setItem(DASHBOARD_ORDER_KEY, JSON.stringify(ids));
  } catch {}
}

// Sort pinned plugins by the saved order; anything not yet ranked (newly
// pinned) keeps its natural order at the end.
function applyOrder(
  pinned: PluginInstance[],
  order: string[],
): PluginInstance[] {
  const rank = new Map(order.map((id, index) => [id, index]));
  return [...pinned].sort((a, b) => {
    const ra = rank.get(a.id) ?? Number.MAX_SAFE_INTEGER;
    const rb = rank.get(b.id) ?? Number.MAX_SAFE_INTEGER;
    return ra - rb;
  });
}

export default function DashboardPage() {
  const plugins = useAppStore((s) => s.plugins);
  const refreshRuntimeState = useAppStore((s) => s.refreshRuntimeState);
  // Hydrate from localStorage lazily. On the server this is []; the first
  // client render also produces an empty pinned grid (plugins load after
  // mount), so applying a different order here cannot cause a hydration
  // mismatch.
  const [order, setOrder] = useState<string[]>(() =>
    typeof window === "undefined" ? [] : loadDashboardOrder(),
  );

  useEffect(() => {
    const id = setInterval(() => {
      void refreshRuntimeState();
    }, 3_000);
    return () => clearInterval(id);
  }, [refreshRuntimeState]);

  const pinnedPlugins = useMemo(
    () =>
      applyOrder(
        plugins.filter((p) => p.pinned),
        order,
      ),
    [plugins, order],
  );

  const handleReorder = (ids: string[]) => {
    setOrder(ids);
    saveDashboardOrder(ids);
  };

  return (
    <>
      <AppHeader title="仪表盘" />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="space-y-8">
          <section>
            <h2 className="text-lg font-semibold mb-4">系统概览</h2>
            <SystemMetrics />
          </section>

          <section>
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-lg font-semibold">
                固定的插件
                <span className="text-muted-foreground font-normal ml-2 text-sm">
                  ({pinnedPlugins.length})
                </span>
              </h2>
              <Button variant="ghost" size="sm" asChild>
                <Link href="/plugins">
                  查看全部
                  <ArrowRight className="h-4 w-4 ml-1" />
                </Link>
              </Button>
            </div>
            {pinnedPlugins.length > 0 ? (
              <SortablePluginGrid
                plugins={pinnedPlugins}
                onReorder={handleReorder}
              />
            ) : (
              <div className="border border-dashed rounded-lg p-8 text-center text-muted-foreground">
                <p>还没有固定的插件</p>
                <p className="text-sm mt-1">
                  在插件中心点击插件卡片的菜单，选择&ldquo;固定到仪表盘&rdquo;
                </p>
              </div>
            )}
          </section>
        </div>
      </main>
    </>
  );
}
