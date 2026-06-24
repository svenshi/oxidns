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
import { useUpdateStore } from "@/lib/update-store";
import { AppHeader } from "@/components/shell/app-header";
import {
  ConnectionRequired,
  ConnectionPending,
  LoginRequired,
} from "@/components/shell/connection-required";
import { RestartingOverlay } from "@/components/shell/restarting-overlay";
import { UpgradeOverlay } from "@/components/shell/upgrade-overlay";
import { TooltipProvider } from "@/components/ui/tooltip";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

export default function ConsoleLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const { t } = useI18n();
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
  const checkForUpdates = useUpdateStore((s) => s.checkForUpdates);
  const resetApplyState = useUpdateStore((s) => s.resetApplyState);
  const upgradeAutoCheck = useUpdateStore((s) => s.upgradeConfig.autoCheck);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const sidebarStateBeforeEditor = useRef(sidebarOpen);
  const previousEditorMode = useRef(editorMode);
  const hasCheckedUpdates = useRef(false);

  // Once the store has hydrated, eagerly probe the configured backend (default
  // `/api`). Only fall back to the connection prompt if that attempt fails.
  useEffect(() => {
    if (!isAuthHydrated) return;
    void attemptAutoConnect();
  }, [isAuthHydrated, attemptAutoConnect]);

  // While the initial auto-connect is still in flight, neither render
  // backend-dependent pages nor the connection-required prompt; show a pending state.
  const isAutoConnectPending =
    isAuthHydrated &&
    !isConnected &&
    (!hasAttemptedAutoConnect || (isConnecting && !connectionError));
  const canUseBackendPages =
    !isAuthHydrated || isConnected || pathname === "/settings";

  useEffect(() => {
    if (isConnected) void loadConfig();
  }, [isConnected, loadConfig]);

  const health = useAppStore((s) => s.health);
  const system = useAppStore((s) => s.system);

  useEffect(() => {
    if (!isConnected || hasCheckedUpdates.current || !upgradeAutoCheck) return;
    if (!buildInfo?.enabled_features.includes("plugin-upgrade")) return;
    const version = system?.version ?? health?.version;
    if (!version) return;
    hasCheckedUpdates.current = true;
    void checkForUpdates(version);
  }, [
    isConnected,
    upgradeAutoCheck,
    health,
    system,
    buildInfo,
    checkForUpdates,
  ]);

  // On disconnect: allow auto-check to re-fire on the next reconnect, and
  // clear any in-progress upgrade state (the server restarted or the apply failed).
  useEffect(() => {
    if (!isConnected) {
      hasCheckedUpdates.current = false;
      resetApplyState();
    }
  }, [isConnected, resetApplyState]);

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
              <AppHeader title={t(WEBUI.shell.configEditor)} />
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
              <AppHeader title={t(WEBUI.shell.connectBackend)} />
              <ConnectionPending />
            </>
          ) : needsCredentials ? (
            <>
              <AppHeader title={t(WEBUI.shell.login)} />
              <LoginRequired />
            </>
          ) : (
            <>
              <AppHeader title={t(WEBUI.shell.connectBackend)} />
              <ConnectionRequired />
            </>
          )}
        </SidebarInset>
        <PluginDetailSheet />
        <ConfigHistorySheet open={historyOpen} onOpenChange={setHistoryOpen} />
        <RestartingOverlay />
        <UpgradeOverlay />
      </SidebarProvider>
    </TooltipProvider>
  );
}
