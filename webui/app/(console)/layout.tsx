"use client";

import { useEffect, useRef, useState } from "react";
import { usePathname } from "next/navigation";
import { SidebarProvider, SidebarInset } from "@/components/ui/sidebar";
import { AppSidebar } from "@/components/shell/app-sidebar";
import { PluginDetailSheet } from "@/components/plugins/plugin-detail-sheet";
import { ConfigEditorView } from "@/components/config/config-editor-view";
import { OfflineConfigImport } from "@/components/config/offline-config-import";
import { ConfigHistorySheet } from "@/components/config/config-history-sheet";
import { useAppStore } from "@/lib/store";
import { useAuthStore } from "@/lib/auth-store";
import { AppHeader } from "@/components/shell/app-header";
import {
  ConnectionRequired,
  ConnectionPending,
  LoginRequired,
} from "@/components/shell/connection-required";
import { RestartingOverlay } from "@/components/shell/restarting-overlay";
import { TooltipProvider } from "@/components/ui/tooltip";

export default function ConsoleLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const editorMode = useAppStore((s) => s.editorMode);
  const historyOpen = useAppStore((s) => s.historyOpen);
  const setHistoryOpen = useAppStore((s) => s.setHistoryOpen);
  const loadConfig = useAppStore((s) => s.loadConfig);
  const refreshMetrics = useAppStore((s) => s.refreshMetrics);
  const isOfflineMode = useAppStore((s) => s.isOfflineMode);
  const exitOfflineMode = useAppStore((s) => s.exitOfflineMode);
  const isConnected = useAuthStore((s) => s.isConnected);
  const isConnecting = useAuthStore((s) => s.isConnecting);
  const connectionError = useAuthStore((s) => s.connectionError);
  const needsCredentials = useAuthStore((s) => s.needsCredentials);
  const hasAttemptedAutoConnect = useAuthStore(
    (s) => s.hasAttemptedAutoConnect,
  );
  const attemptAutoConnect = useAuthStore((s) => s.attemptAutoConnect);
  const isAuthHydrated = useAuthStore((s) => s.isHydrated);
  const pathname = usePathname();
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const sidebarStateBeforeEditor = useRef(sidebarOpen);
  const previousEditorMode = useRef(editorMode);

  // Once the store has hydrated, eagerly probe the configured backend (default
  // `/api`). Only fall back to the connection prompt if that attempt fails.
  useEffect(() => {
    if (!isAuthHydrated) return;
    void attemptAutoConnect();
  }, [isAuthHydrated, attemptAutoConnect]);

  // While the initial auto-connect is still in flight, neither render
  // backend-dependent pages nor the "需要连接" prompt; show a pending state.
  const isAutoConnectPending =
    isAuthHydrated &&
    !isConnected &&
    (!hasAttemptedAutoConnect || (isConnecting && !connectionError));
  const canUseBackendPages =
    !isAuthHydrated || isConnected || pathname === "/settings";

  useEffect(() => {
    if (isConnected) void loadConfig();
  }, [isConnected, loadConfig]);

  // On reconnect, drop offline mode so loadConfig's authoritative state wins.
  useEffect(() => {
    if (isConnected && isOfflineMode) exitOfflineMode();
  }, [isConnected, isOfflineMode, exitOfflineMode]);

  // Keep plugin metrics live across the whole console (cards + detail sheet),
  // not just on the dashboard's runtime-state poll.
  useEffect(() => {
    if (!isConnected) return;
    const id = setInterval(() => {
      void refreshMetrics();
    }, 5_000);
    return () => clearInterval(id);
  }, [isConnected, refreshMetrics]);

  useEffect(() => {
    const el = document.documentElement;
    if (editorMode) {
      el.style.overflow = "hidden";
    } else {
      el.style.overflow = "";
    }
    return () => {
      el.style.overflow = "";
    };
  }, [editorMode]);

  useEffect(() => {
    if (!previousEditorMode.current && editorMode) {
      sidebarStateBeforeEditor.current = sidebarOpen;
      setSidebarOpen(false);
    }

    if (previousEditorMode.current && !editorMode) {
      setSidebarOpen(sidebarStateBeforeEditor.current);
    }

    previousEditorMode.current = editorMode;
  }, [editorMode, sidebarOpen]);

  return (
    <TooltipProvider>
      <SidebarProvider
        className="h-svh overflow-hidden"
        open={editorMode ? false : sidebarOpen}
        onOpenChange={(open) => {
          if (!editorMode) {
            setSidebarOpen(open);
          }
        }}
      >
        <AppSidebar />
        <SidebarInset className="h-svh min-h-0 overflow-hidden md:h-[calc(100svh-1rem)]">
          {editorMode ? (
            <div className="flex h-full min-h-0 flex-col overflow-hidden">
              <AppHeader title="配置编辑器" />
              {!isAuthHydrated || isConnected || isOfflineMode ? (
                <ConfigEditorView />
              ) : (
                <OfflineConfigImport />
              )}
            </div>
          ) : canUseBackendPages ? (
            children
          ) : isAutoConnectPending ? (
            <>
              <AppHeader title="连接后台服务" />
              <ConnectionPending />
            </>
          ) : needsCredentials ? (
            <>
              <AppHeader title="登录" />
              <LoginRequired />
            </>
          ) : (
            <>
              <AppHeader title="连接后台服务" />
              <ConnectionRequired />
            </>
          )}
        </SidebarInset>
        <PluginDetailSheet />
        <ConfigHistorySheet open={historyOpen} onOpenChange={setHistoryOpen} />
        <RestartingOverlay />
      </SidebarProvider>
    </TooltipProvider>
  );
}
