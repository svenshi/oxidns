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
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

interface AppHeaderProps {
  title: string;
  breadcrumbs?: { label: string; href?: string }[];
}

export function AppHeader({ title, breadcrumbs = [] }: AppHeaderProps) {
  const { theme, setTheme } = useTheme();
  const { locale, t, toggleLocale } = useI18n();
  const router = useRouter();
  const editorMode = useAppStore((s) => s.editorMode);
  const setEditorMode = useAppStore((s) => s.setEditorMode);
  const webUiMode = useAppStore((s) => s.webUiMode);
  const isConnected = useAuthStore((s) => s.isConnected);
  const updateInfo = useUpdateStore((s) => s.updateInfo);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const backendSupportsUpgrade =
    buildInfo != null
      ? buildInfo.enabled_features.includes("plugin-upgrade")
      : null;
  const showUpgradeNotice =
    isConnected &&
    backendSupportsUpgrade === true &&
    updateInfo?.updateAvailable === true;
  const showNavigation = !editorMode;
  const showEditorToggle = webUiMode !== "standard" || editorMode;
  const homeHref = webUiMode === "standard" ? "/standard" : "/";
  const editorTooltip = editorMode
    ? t(WEBUI.shell.switchToConsole)
    : t(WEBUI.shell.switchToEditor);

  const handleEditorToggle = () => {
    if (editorMode) {
      setEditorMode(false);
      return;
    }
    if (webUiMode === "standard") {
      return;
    }
    setEditorMode(true);
  };

  return (
    <header className="flex h-14 shrink-0 items-center gap-3 border-b px-4">
      {showNavigation ? (
        <>
          <div className="flex min-w-0 items-center gap-2.5">
            <SidebarTrigger className="rounded-md text-muted-foreground hover:text-foreground" />
          </div>
          <Breadcrumb className="min-w-0 flex-1">
            <BreadcrumbList className="gap-2 text-[13px]">
              <BreadcrumbItem>
                <BreadcrumbLink asChild className="text-foreground/70">
                  <Link href={homeHref}>OxiDNS</Link>
                </BreadcrumbLink>
              </BreadcrumbItem>
              {breadcrumbs.map((crumb, i) => (
                <span key={i} className="contents">
                  <BreadcrumbSeparator />
                  <BreadcrumbItem>
                    {crumb.href ? (
                      <BreadcrumbLink asChild className="text-foreground/70">
                        <Link href={crumb.href}>{crumb.label}</Link>
                      </BreadcrumbLink>
                    ) : (
                      <BreadcrumbPage>{crumb.label}</BreadcrumbPage>
                    )}
                  </BreadcrumbItem>
                </span>
              ))}
              <BreadcrumbSeparator />
              <BreadcrumbItem>
                <BreadcrumbPage>{title}</BreadcrumbPage>
              </BreadcrumbItem>
            </BreadcrumbList>
          </Breadcrumb>
        </>
      ) : (
        <div className="min-w-0 flex-1">
          <h1 className="text-sm font-medium text-foreground">{title}</h1>
        </div>
      )}

      <div className="ml-auto flex items-center gap-2">
        <ConfigSyncControl />
        {showUpgradeNotice && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                className="relative rounded-md text-muted-foreground hover:text-foreground"
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
        <div className="flex items-center rounded-lg border border-border/60 bg-background/80 p-0.5 shadow-sm">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                className="rounded-md"
                onClick={toggleLocale}
                aria-label={t(
                  locale === "zh-CN"
                    ? WEBUI.locale.toggleToEnglish
                    : WEBUI.locale.toggleToChinese,
                )}
              >
                <Languages className="h-4 w-4" />
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

          {showEditorToggle && (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant={editorMode ? "secondary" : "ghost"}
                  size="icon-sm"
                  className="rounded-md"
                  onClick={handleEditorToggle}
                >
                  {editorMode ? (
                    <LayoutDashboard className="h-4 w-4" />
                  ) : (
                    <Code2 className="h-4 w-4" />
                  )}
                  <span className="sr-only">
                    {editorMode
                      ? t(WEBUI.shell.consoleMode)
                      : t(WEBUI.shell.editorMode)}
                  </span>
                </Button>
              </TooltipTrigger>
              <TooltipContent>{editorTooltip}</TooltipContent>
            </Tooltip>
          )}

          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                className="rounded-md"
                onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
              >
                <Sun className="h-4 w-4 rotate-0 scale-100 transition-all dark:-rotate-90 dark:scale-0" />
                <Moon className="absolute h-4 w-4 rotate-90 scale-0 transition-all dark:rotate-0 dark:scale-100" />
                <span className="sr-only">{t(WEBUI.shell.toggleTheme)}</span>
              </Button>
            </TooltipTrigger>
            <TooltipContent>{t(WEBUI.shell.toggleTheme)}</TooltipContent>
          </Tooltip>
        </div>
      </div>
    </header>
  );
}
