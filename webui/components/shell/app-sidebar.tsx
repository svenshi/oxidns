"use client";

import Image from "next/image";
import Link from "next/link";
import { usePathname } from "next/navigation";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarGroupContent,
} from "@/components/ui/sidebar";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useAuthStore } from "@/lib/auth-store";
import { useAppStore } from "@/lib/store";
import {
  Activity,
  BookOpen,
  GitBranch,
  LogOut,
  Puzzle,
  ScrollText,
  Settings,
  User,
  Radio,
} from "lucide-react";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";

const navItems = [
  {
    titleKey: WEBUI.shell.dashboard,
    href: "/",
    icon: Activity,
    badge: "实时",
  },
  {
    titleKey: WEBUI.shell.plugins,
    href: "/plugins",
    icon: Puzzle,
  },
  {
    titleKey: WEBUI.shell.logs,
    href: "/logs",
    icon: ScrollText,
  },
  {
    titleKey: WEBUI.shell.settings,
    href: "/settings",
    icon: Settings,
  },
];

export function AppSidebar() {
  const { t } = useI18n();
  const pathname = usePathname();
  const isConnected = useAuthStore((s) => s.isConnected);
  const serverConfig = useAuthStore((s) => s.serverConfig);
  const logout = useAuthStore((s) => s.logout);
  const isOfflineMode = useAppStore((s) => s.isOfflineMode);

  return (
    <Sidebar variant="inset" className="bg-transparent">
      {/* Brand Header */}
      <SidebarHeader className="h-16 justify-center border-b border-white/20 dark:border-white/10 px-4">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton asChild className="h-11 rounded-xl px-2.5 hover:bg-white/10 dark:hover:bg-white/5 transition-all">
              <Link href="/" className="flex items-center gap-2.5">
                <div className="relative size-8 shrink-0 flex items-center justify-center rounded-xl bg-primary/10 border border-primary/20 shadow-xs">
                  <Image
                    src="/logo-light.png"
                    alt="OxiDNS"
                    width={22}
                    height={22}
                    className="size-5 object-contain dark:hidden"
                    priority
                  />
                  <Image
                    src="/logo-dark.png"
                    alt="OxiDNS"
                    width={22}
                    height={22}
                    className="hidden size-5 object-contain dark:block"
                    priority
                  />
                </div>
                <span className="font-bold text-base tracking-tight text-foreground">
                  OxiDNS
                </span>
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      {/* Navigation Links */}
      <SidebarContent className="px-3 py-4">
        <SidebarGroup>
          <SidebarGroupLabel className="text-[10px] tracking-wider uppercase font-semibold text-muted-foreground/70 px-2 mb-1.5">
            {t(WEBUI.shell.navigation)}
          </SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu className="space-y-1.5">
              {navItems.map((item) => {
                const isActive = pathname === item.href;
                return (
                  <SidebarMenuItem key={item.href}>
                    <SidebarMenuButton
                      asChild
                      isActive={isActive}
                      className={cn(
                        "h-10 rounded-xl px-3 transition-all duration-200 text-xs font-medium",
                        isActive
                          ? "liquid-glass-active text-primary font-semibold shadow-sm"
                          : "hover:bg-sidebar-accent/60 text-sidebar-foreground",
                      )}
                    >
                      <Link href={item.href} className="flex items-center justify-between w-full">
                        <div className="flex items-center gap-3">
                          <item.icon
                            className={cn(
                              "size-4 shrink-0 transition-transform duration-200",
                              isActive ? "text-primary scale-110" : "text-muted-foreground",
                            )}
                          />
                          <span>{t(item.titleKey)}</span>
                        </div>
                        {item.badge && (
                          <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-foreground/[0.06] text-muted-foreground font-mono">
                            {item.badge}
                          </span>
                        )}
                      </Link>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                );
              })}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      {/* Footer Info Card */}
      <SidebarFooter className="border-t border-border/40 p-3 space-y-2.5">
        <div className="p-3 rounded-2xl liquid-glass-panel text-xs space-y-1.5">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground font-medium">运行状态</span>
            {isConnected ? (
              <span className="flex items-center gap-1.5 text-emerald-500 font-medium font-mono text-[11px]">
                <span className="h-1.5 w-1.5 rounded-full bg-emerald-500 animate-pulse" />
                已联机
              </span>
            ) : isOfflineMode ? (
              <span className="flex items-center gap-1.5 text-amber-500 font-medium font-mono text-[11px]">
                <Radio className="h-3 w-3" />
                本地配置
              </span>
            ) : (
              <span className="flex items-center gap-1.5 text-muted-foreground font-mono text-[11px]">
                <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground/50" />
                待连接
              </span>
            )}
          </div>
          <div className="flex items-center justify-between text-muted-foreground text-[11px]">
            <span>端口: :53 (TCP/UDP)</span>
            <span className="font-mono">:9199</span>
          </div>
        </div>

        <SidebarMenu>
          {isConnected && serverConfig.requiresAuth && (
            <SidebarMenuItem>
              <div className="flex items-center justify-between gap-1 px-2.5 py-1">
                <span className="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
                  <User className="size-3.5 shrink-0" />
                  <span className="truncate">{serverConfig.username}</span>
                </span>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      onClick={logout}
                      className="flex size-6 shrink-0 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
                    >
                      <LogOut className="size-3.5" />
                      <span className="sr-only">{t(WEBUI.shell.logout)}</span>
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="right">
                    {t(WEBUI.shell.logout)}
                  </TooltipContent>
                </Tooltip>
              </div>
            </SidebarMenuItem>
          )}

          <div className="flex items-center justify-between px-1 text-xs text-muted-foreground pt-1">
            <a
              href="https://oxidns.org"
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center gap-1.5 hover:text-foreground transition-colors"
            >
              <BookOpen className="size-3.5" />
              <span>文档</span>
            </a>
            <a
              href="https://github.com/encounter0v0/oxidns"
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center gap-1.5 hover:text-foreground transition-colors"
            >
              <GitBranch className="size-3.5" />
              <span>GitHub</span>
            </a>
          </div>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
}
