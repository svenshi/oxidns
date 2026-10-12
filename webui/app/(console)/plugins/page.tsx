/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { Suspense, useState } from "react";
import { useSearchParams } from "next/navigation";
import { AppHeader } from "@/components/shell/app-header";
import { SortablePluginGrid } from "@/components/plugins/sortable-plugin-grid";
import { CreatePluginDialog } from "@/components/plugins/create-plugin-dialog";
import { PluginDeleteButton } from "@/components/plugins/plugin-delete-button";
import { useAppStore } from "@/lib/store";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Search, LayoutGrid, List, Pin, PinOff, GitBranch } from "lucide-react";
import type { PluginType } from "@/lib/types";
import { isPluginKindSupported } from "@/lib/build-capabilities";
import { cn } from "@/lib/utils";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  getPluginCatalogItem,
  renderPluginKindIcon,
} from "@/components/plugins/catalog";
import {
  pluginTypeColors,
  pluginTypeIcons,
} from "@/components/plugins/display";
import { TopologyView } from "@/components/plugins/plugin-topology-view";
import { LOCALES, WEBUI } from "@/lib/i18n";
import {
  getPluginSearchText,
  pluginTypeLabel,
} from "@/lib/i18n/plugin-defined";
import { useI18n } from "@/lib/i18n/provider";

export default function PluginsPage() {
  return (
    <Suspense fallback={<PluginsPageFallback />}>
      <PluginsPageContent />
    </Suspense>
  );
}

function PluginsPageContent() {
  const { locale, t } = useI18n();
  const searchParams = useSearchParams();
  const initialType = searchParams.get("type") as PluginType | null;
  const [activeTab, setActiveTab] = useState<PluginType | "all">(
    initialType || "all",
  );
  const [viewMode, setViewMode] = useState<"grid" | "table" | "topology">(
    "grid",
  );
  const [search, setSearch] = useState("");

  const plugins = useAppStore((s) => s.plugins);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const dependencyGraph = useAppStore((s) => s.dependencyGraph);
  const { setSelectedPlugin, setDetailOpen, togglePluginPin, reorderPlugins } =
    useAppStore();

  const filteredPlugins = plugins.filter((p) => {
    const definition = getPluginCatalogItem(p.pluginKind, locale);
    const baseDefinition = getPluginCatalogItem(p.pluginKind);
    const normalizedSearch = search.toLowerCase();
    const matchesType = activeTab === "all" || p.type === activeTab;
    const searchableText = [
      p.name,
      p.pluginKind,
      p.type,
      pluginTypeLabel(p.type, locale),
      definition?.name,
      definition?.description,
      baseDefinition
        ? getPluginSearchText(baseDefinition, [...LOCALES])
        : undefined,
    ]
      .filter((text): text is string => Boolean(text))
      .join(" ")
      .toLowerCase();
    const matchesSearch = searchableText.includes(normalizedSearch);
    return matchesType && matchesSearch;
  });

  const pluginsByType = {
    server: plugins.filter((p) => p.type === "server"),
    executor: plugins.filter((p) => p.type === "executor"),
    matcher: plugins.filter((p) => p.type === "matcher"),
    provider: plugins.filter((p) => p.type === "provider"),
  };

  const handleRowClick = (plugin: (typeof plugins)[0]) => {
    setSelectedPlugin(plugin);
    setDetailOpen(true);
  };

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <AppHeader title={t(WEBUI.plugins.centerTitle)} />
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <Tabs
          value={activeTab}
          onValueChange={(v) => setActiveTab(v as PluginType | "all")}
          className="flex min-h-0 flex-1 flex-col"
        >
          {/* Fixed toolbar + tab headers */}
          <div className="shrink-0 space-y-4 border-b px-6 pt-5 pb-4">
            <div className="flex items-center justify-between gap-4 flex-wrap">
              <div className="flex items-center gap-3 flex-1 min-w-[200px] max-w-md">
                <div className="relative flex-1">
                  <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                  <Input
                    placeholder={t(WEBUI.plugins.searchPlaceholder)}
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
                    className="pl-9"
                  />
                </div>
              </div>
              <div className="flex items-center gap-2">
                <div className="flex items-center border rounded-md">
                  <Button
                    variant={viewMode === "grid" ? "secondary" : "ghost"}
                    size="sm"
                    className="rounded-r-none"
                    onClick={() => setViewMode("grid")}
                  >
                    <LayoutGrid className="h-4 w-4" />
                  </Button>
                  <Button
                    variant={viewMode === "table" ? "secondary" : "ghost"}
                    size="sm"
                    className="rounded-l-none rounded-r-none"
                    onClick={() => setViewMode("table")}
                  >
                    <List className="h-4 w-4" />
                  </Button>
                  <Button
                    variant={viewMode === "topology" ? "secondary" : "ghost"}
                    size="sm"
                    className="rounded-l-none"
                    onClick={() => setViewMode("topology")}
                  >
                    <GitBranch className="h-4 w-4" />
                  </Button>
                </div>
                <CreatePluginDialog
                  defaultType={activeTab !== "all" ? activeTab : undefined}
                />
              </div>
            </div>

            {viewMode !== "topology" && (
              // The category bar overflows narrow (mobile) viewports, so make
              // it horizontally scrollable. -mx-6 px-6 lets the pills bleed to
              // the screen edges while staying aligned with the toolbar inset;
              // on desktop the list fits and nothing scrolls.
              <div className="oxidns-no-scrollbar -mx-6 overflow-x-auto px-6">
                <TabsList>
                  <TabsTrigger value="all">
                    {t(WEBUI.common.all)}
                    <Badge variant="secondary" className="ml-1.5 text-xs">
                      {plugins.length}
                    </Badge>
                  </TabsTrigger>
                  {(Object.keys(pluginsByType) as PluginType[]).map((type) => (
                    <TabsTrigger key={type} value={type} className="gap-1.5">
                      {pluginTypeIcons[type]}
                      {pluginTypeLabel(type, locale)}
                      <Badge variant="secondary" className="ml-1 text-xs">
                        {pluginsByType[type].length}
                      </Badge>
                    </TabsTrigger>
                  ))}
                </TabsList>
              </div>
            )}
          </div>

          {/* Scrollable content area */}
          <div className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto">
            <TabsContent value={activeTab} className="m-0 p-6">
              {viewMode === "grid" ? (
                <SortablePluginGrid
                  plugins={filteredPlugins}
                  onReorder={(ids) => void reorderPlugins(ids)}
                  disabled={search.trim() !== ""}
                />
              ) : viewMode === "table" ? (
                <div className="border rounded-lg overflow-hidden">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>{t(WEBUI.common.name)}</TableHead>
                        <TableHead>{t(WEBUI.common.type)}</TableHead>
                        <TableHead>{t(WEBUI.common.plugin)}</TableHead>
                        <TableHead className="w-[80px]" />
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {filteredPlugins.map((plugin) => (
                        <TableRow
                          key={plugin.id}
                          className="group cursor-pointer"
                          onClick={() => handleRowClick(plugin)}
                        >
                          <TableCell className="font-mono font-medium">
                            <div className="flex items-center gap-2">
                              {plugin.name}
                              {plugin.pinned && (
                                <Pin className="h-3 w-3 text-primary" />
                              )}
                            </div>
                          </TableCell>
                          <TableCell>
                            <Badge
                              variant="outline"
                              className={cn(
                                "gap-1",
                                pluginTypeColors[plugin.type],
                              )}
                            >
                              {pluginTypeIcons[plugin.type]}
                              {pluginTypeLabel(plugin.type, locale)}
                            </Badge>
                          </TableCell>
                          <TableCell>
                            <PluginKindBadge
                              pluginKind={plugin.pluginKind}
                              supported={isPluginKindSupported(
                                buildInfo,
                                plugin.type,
                                plugin.pluginKind,
                              )}
                            />
                          </TableCell>
                          <TableCell>
                            <div className="flex items-center justify-end gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                              <Tooltip>
                                <TooltipTrigger asChild>
                                  <Button
                                    variant="ghost"
                                    size="icon"
                                    className={cn(
                                      "h-7 w-7",
                                      plugin.pinned && "text-primary",
                                    )}
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      togglePluginPin(plugin.id);
                                    }}
                                  >
                                    {plugin.pinned ? (
                                      <PinOff className="h-3.5 w-3.5" />
                                    ) : (
                                      <Pin className="h-3.5 w-3.5" />
                                    )}
                                  </Button>
                                </TooltipTrigger>
                                <TooltipContent side="bottom">
                                  {plugin.pinned
                                    ? t(WEBUI.plugins.unpinDashboard)
                                    : t(WEBUI.plugins.pinDashboard)}
                                </TooltipContent>
                              </Tooltip>
                              <PluginDeleteButton
                                plugin={plugin}
                                className="h-7 w-7 hover:text-destructive"
                              />
                            </div>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              ) : (
                <TopologyView
                  plugins={plugins}
                  dependencyGraph={dependencyGraph}
                  onSelect={handleRowClick}
                />
              )}

              {viewMode !== "topology" && filteredPlugins.length === 0 && (
                <div className="border border-dashed rounded-lg p-12 text-center text-muted-foreground">
                  <p>{t(WEBUI.plugins.noMatches)}</p>
                  {search && (
                    <p className="text-sm mt-1">
                      {t(WEBUI.plugins.tryAdjustSearch)}
                      <button
                        onClick={() => setSearch("")}
                        className="text-primary hover:underline ml-1"
                      >
                        {t(WEBUI.common.clearSearch)}
                      </button>
                    </p>
                  )}
                </div>
              )}
            </TabsContent>
          </div>
        </Tabs>
      </div>
    </div>
  );
}

function PluginsPageFallback() {
  const { t } = useI18n();
  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <AppHeader title={t(WEBUI.plugins.centerTitle)} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="rounded-lg border border-dashed p-12 text-center text-sm text-muted-foreground">
          {t(WEBUI.common.loading)}
        </div>
      </main>
    </div>
  );
}

function PluginKindBadge({
  pluginKind,
  supported,
}: {
  pluginKind: string;
  supported: boolean;
}) {
  const { locale, t } = useI18n();
  const definition = getPluginCatalogItem(pluginKind, locale);

  return (
    <Badge
      variant="outline"
      className={cn(
        "gap-1.5",
        !supported && "border-dashed text-muted-foreground",
      )}
    >
      {definition &&
        renderPluginKindIcon(definition.icon, { className: "h-3 w-3" })}
      {definition?.name ?? pluginKind}
      {!supported && ` · ${t(WEBUI.common.notCompiled)}`}
    </Badge>
  );
}
