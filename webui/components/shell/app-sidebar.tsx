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
import {
  BookOpen,
  GitBranch,
  LayoutDashboard,
  Activity,
  LogOut,
  Puzzle,
  ScrollText,
  Settings,
  User,
} from "lucide-react";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

const navItems = [
  {
    titleKey: WEBUI.shell.observability,
    href: "/observability",
    icon: Activity,
  },
  {
    titleKey: WEBUI.shell.dashboard,
    href: "/",
    icon: LayoutDashboard,
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

  return (
    <Sidebar variant="inset">
      <SidebarHeader className="h-14 justify-center border-b border-sidebar-border px-3 py-1">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton asChild className="h-9 rounded-md px-2">
              <Link href="/">
                <div className="relative size-8 shrink-0">
                  <Image
                    src="/logo-light.png"
                    alt="OxiDNS"
                    width={32}
                    height={32}
                    className="size-8 object-contain dark:hidden"
                    priority
                  />
                  <Image
                    src="/logo-dark.png"
                    alt="OxiDNS"
                    width={32}
                    height={32}
                    className="hidden size-8 object-contain dark:block"
                    priority
                  />
                </div>
                <div className="flex flex-col gap-0.5 leading-none">
                  <span className="font-semibold">OxiDNS</span>
                  <span className="text-xs text-muted-foreground">
                    {t(WEBUI.shell.console)}
                  </span>
                </div>
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>{t(WEBUI.shell.navigation)}</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {navItems.map((item) => (
                <SidebarMenuItem key={item.href}>
                  <SidebarMenuButton asChild isActive={pathname === item.href}>
                    <Link href={item.href}>
                      <item.icon className="size-4" />
                      <span>{t(item.titleKey)}</span>
                    </Link>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter className="border-t border-sidebar-border">
        <SidebarMenu>
          {isConnected && serverConfig.requiresAuth && (
            <SidebarMenuItem>
              <div className="flex items-center justify-between gap-1 px-2 py-1">
                <span className="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
                  <User className="size-3.5 shrink-0" />
                  <span className="truncate">{serverConfig.username}</span>
                </span>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      onClick={logout}
                      className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
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
          <SidebarMenuItem>
            <SidebarMenuButton asChild>
              <a
                href="https://oxidns.org"
                target="_blank"
                rel="noopener noreferrer"
                className="text-muted-foreground"
              >
                <BookOpen className="size-4" />
                <span>{t(WEBUI.shell.docs)}</span>
              </a>
            </SidebarMenuButton>
          </SidebarMenuItem>
          <SidebarMenuItem>
            <SidebarMenuButton asChild>
              <a
                href="https://github.com/svenshi/oxidns"
                target="_blank"
                rel="noopener noreferrer"
                className="text-muted-foreground"
              >
                <GitBranch className="size-4" />
                <span>GitHub</span>
              </a>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
}
