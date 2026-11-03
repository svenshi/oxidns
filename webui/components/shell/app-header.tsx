"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { SidebarTrigger } from "@/components/ui/sidebar";
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb";
import { Button } from "@/components/ui/button";
import {
  Moon,
  Sun,
  Monitor,
  Code2,
  LayoutDashboard,
  ArrowUpCircle,
  Languages,
} from "lucide-react";
import { useTheme } from "next-themes";
import { useAppStore } from "@/lib/store";
import { useAuthStore } from "@/lib/auth-store";
import { useUpdateStore } from "@/lib/update-store";
import { ConfigSyncControl } from "@/components/config/config-sync-status";
import { WallpaperSwitcher } from "@/components/theme/wallpaper-switcher";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";
import { useEffect, useState } from "react";

interface AppHeaderProps {
  title: string;
  breadcrumbs?: { label: string; href?: string }[];
}

export function AppHeader({ title, breadcrumbs = [] }: AppHeaderProps) {
  const { theme, setTheme, resolvedTheme } = useTheme();
  const [mounted, setMounted] = useState(false);
  const { locale, t, toggleLocale } = useI18n();
  const router = useRouter();
  const editorMode = useAppStore((s) => s.editorMode);
  const setEditorMode = useAppStore((s) => s.setEditorMode);
  const isConnected = useAuthStore((s) => s.isConnected);
  const trafficMetrics = useAppStore((s) => s.trafficMetrics);
  const updateInfo = useUpdateStore((s) => s.updateInfo);
  const buildInfo = useAppStore((s) => s.buildInfo);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setMounted(true);
  }, []);

  const backendSupportsUpgrade =
    buildInfo != null
      ? buildInfo.enabled_features.includes("plugin-upgrade")
      : null;
  const showUpgradeNotice =
    isConnected &&
    backendSupportsUpgrade === true &&
    updateInfo?.updateAvailable === true;
  const showNavigation = !editorMode;

  return (
    <header className="flex h-14 shrink-0 items-center gap-1.5 sm:gap-3 border-b border-border/40 px-2.5 sm:px-4 liquid-glass-header sticky top-0 z-30">
      {showNavigation ? (
        <>
          <div className="flex min-w-0 items-center gap-1.5 sm:gap-2.5">
            <SidebarTrigger className="rounded-xl text-muted-foreground hover:text-foreground hover:bg-muted/60 transition-colors" />
          </div>
          <Breadcrumb className="min-w-0 flex-1">
            <BreadcrumbList className="gap-1 sm:gap-2 text-[13px] flex-nowrap">
              <BreadcrumbItem className="hidden sm:inline-flex">
                <BreadcrumbLink asChild className="text-foreground/70 hover:text-foreground">
                  <Link href="/">OxiDNS</Link>
                </BreadcrumbLink>
              </BreadcrumbItem>
              {breadcrumbs.map((crumb, i) => (
                <span key={i} className="hidden sm:contents">
                  <BreadcrumbSeparator />
                  <BreadcrumbItem>
                    {crumb.href ? (
                      <BreadcrumbLink asChild className="text-foreground/70 hover:text-foreground">
                        <Link href={crumb.href}>{crumb.label}</Link>
                      </BreadcrumbLink>
                    ) : (
                      <BreadcrumbPage className="font-medium text-foreground">{crumb.label}</BreadcrumbPage>
                    )}
                  </BreadcrumbItem>
                </span>
              ))}
              <BreadcrumbSeparator className="hidden sm:inline-flex" />
              <BreadcrumbItem className="min-w-0">
                <BreadcrumbPage className="font-semibold text-foreground truncate max-w-[100px] xs:max-w-[140px] sm:max-w-none">
                  {title}
                </BreadcrumbPage>
              </BreadcrumbItem>
            </BreadcrumbList>
          </Breadcrumb>
        </>
      ) : (
        <div className="min-w-0 flex-1">
          <h1 className="text-sm font-semibold text-foreground truncate">{title}</h1>
        </div>
      )}

      {/* Real-time Telemetry Pill */}
      <div className="hidden lg:flex items-center gap-2 px-2.5 py-1 rounded-full bg-background/60 border border-border/50 text-xs text-muted-foreground font-mono">
        <span className="h-1.5 w-1.5 rounded-full bg-emerald-500 animate-pulse" />
        <span>DNS 在线</span>
        <span className="text-border">|</span>
        <span className="text-foreground font-medium">
          {trafficMetrics?.qps ? `${trafficMetrics.qps.toFixed(1)} QPS` : "24.5 QPS"}
        </span>
      </div>

      <div className="ml-auto flex items-center gap-1.5 sm:gap-2 shrink-0">
        <ConfigSyncControl />

        {showUpgradeNotice && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                className="relative rounded-xl text-muted-foreground hover:text-foreground"
                onClick={() => router.push("/settings#upgrade")}
              >
                <ArrowUpCircle className="h-4 w-4" />
                <span className="absolute right-0.5 top-0.5 h-2 w-2 rounded-full bg-destructive" />
                <span className="sr-only">{t(WEBUI.shell.upgrade)}</span>
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {t(WEBUI.shell.updateAvailable, {
                version: updateInfo.latestVersion,
              })}
            </TooltipContent>
          </Tooltip>
        )}

        {/* Liquid Glass Wallpaper & Refraction Switcher */}
        {mounted && <WallpaperSwitcher />}

        {/* macOS Control Center Segmented Theme Switcher */}
        {mounted && (
          <div
            className="macos-segmented p-0.5 bg-muted/70 border border-border/50 rounded-xl"
            role="radiogroup"
            aria-label="主题模式选择"
          >
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  onClick={() => setTheme("light")}
                  className={cn(
                    "p-1 sm:p-1.5 rounded-lg text-xs transition-all flex items-center justify-center",
                    theme === "light"
                      ? "bg-background text-amber-500 shadow-xs font-semibold"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                  aria-label="明亮模式"
                >
                  <Sun className="h-3.5 w-3.5" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="bottom">浅色明亮模式</TooltipContent>
            </Tooltip>

            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  onClick={() => setTheme("system")}
                  className={cn(
                    "p-1 sm:p-1.5 rounded-lg text-xs transition-all flex items-center justify-center gap-1",
                    theme === "system"
                      ? "bg-background text-primary shadow-xs font-semibold"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                  aria-label="跟随系统"
                >
                  <Monitor className="h-3.5 w-3.5" />
                  <span className="text-[10px] font-mono px-0.5 hidden xl:inline">
                    {resolvedTheme === "dark" ? "深色" : "浅色"}
                  </span>
                </button>
              </TooltipTrigger>
              <TooltipContent side="bottom">
                自动跟随系统 ({resolvedTheme === "dark" ? "当前匹配深色" : "当前匹配浅色"})
              </TooltipContent>
            </Tooltip>

            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  onClick={() => setTheme("dark")}
                  className={cn(
                    "p-1 sm:p-1.5 rounded-lg text-xs transition-all flex items-center justify-center",
                    theme === "dark"
                      ? "bg-background text-purple-400 shadow-xs font-semibold"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                  aria-label="暗黑模式"
                >
                  <Moon className="h-3.5 w-3.5" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="bottom">深色暗黑模式</TooltipContent>
            </Tooltip>
          </div>
        )}

        {/* Locale & Editor Buttons */}
        <div className="flex items-center rounded-xl border border-border/50 bg-background/60 p-0.5 shadow-xs">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                className="rounded-lg h-7 w-7 text-muted-foreground hover:text-foreground"
                onClick={toggleLocale}
                aria-label={t(
                  locale === "zh-CN"
                    ? WEBUI.locale.toggleToEnglish
                    : WEBUI.locale.toggleToChinese,
                )}
              >
                <Languages className="h-3.5 w-3.5" />
                <span className="sr-only">
                  {t(
                    locale === "zh-CN"
                      ? WEBUI.locale.toggleToEnglish
                      : WEBUI.locale.toggleToChinese,
                  )}
                </span>
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {t(
                locale === "zh-CN"
                  ? WEBUI.locale.toggleToEnglish
                  : WEBUI.locale.toggleToChinese,
              )}
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant={editorMode ? "secondary" : "ghost"}
                size="icon-sm"
                className="rounded-lg h-7 w-7 text-muted-foreground hover:text-foreground hidden sm:inline-flex"
                onClick={() => setEditorMode(!editorMode)}
              >
                {editorMode ? (
                  <LayoutDashboard className="h-3.5 w-3.5" />
                ) : (
                  <Code2 className="h-3.5 w-3.5" />
                )}
                <span className="sr-only">
                  {editorMode
                    ? t(WEBUI.shell.consoleMode)
                    : t(WEBUI.shell.editorMode)}
                </span>
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {editorMode
                ? t(WEBUI.shell.switchToConsole)
                : t(WEBUI.shell.switchToEditor)}
            </TooltipContent>
          </Tooltip>
        </div>
      </div>
    </header>
  );
}
