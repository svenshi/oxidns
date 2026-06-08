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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useAuthStore } from "@/lib/auth-store";
import {
  BookOpen,
  GitBranch,
  LayoutDashboard,
  LogOut,
  Puzzle,
  ScrollText,
  Settings,
  User,
} from "lucide-react";

const navItems = [
  {
    title: "仪表盘",
    href: "/",
    icon: LayoutDashboard,
  },
  {
    title: "插件中心",
    href: "/plugins",
    icon: Puzzle,
  },
  {
    title: "运行日志",
    href: "/logs",
    icon: ScrollText,
  },
  {
    title: "系统配置",
    href: "/settings",
    icon: Settings,
  },
];

export function AppSidebar() {
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
                  <span className="text-xs text-muted-foreground">控制台</span>
                </div>
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>导航</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {navItems.map((item) => (
                <SidebarMenuItem key={item.href}>
                  <SidebarMenuButton asChild isActive={pathname === item.href}>
                    <Link href={item.href}>
                      <item.icon className="size-4" />
                      <span>{item.title}</span>
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
                      <span className="sr-only">退出登录</span>
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="right">退出登录</TooltipContent>
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
                <span>文档站</span>
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
