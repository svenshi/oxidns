"use client";

import { useAppStore } from "@/lib/store";
import {
  YamlEditor,
  type YamlEditorHandle,
} from "@/components/config/yaml-editor";
import { PluginIndexPanel } from "@/components/config/plugin-index-panel";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import {
  Save,
  RotateCcw,
  FileCode2,
  CheckCircle2,
  AlertCircle,
  Download,
  Copy,
  ClipboardCheck,
  LogOut,
  PanelRightOpen,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Spinner } from "@/components/ui/spinner";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetDescription,
} from "@/components/ui/sheet";
import { useIsMobile } from "@/hooks/use-mobile";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

export function ConfigEditorView() {
  const { t } = useI18n();
  const yamlConfig = useAppStore((s) => s.configText);
  const configEditorBaseline = useAppStore((s) => s.configEditorBaseline);
  const setYamlConfig = useAppStore((s) => s.setYamlConfig);
  const saveConfig = useAppStore((s) => s.saveConfig);
  const isConfigLoading = useAppStore((s) => s.isConfigLoading);
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const isRestarting = useAppStore((s) => s.isRestarting);
  const configError = useAppStore((s) => s.configError);
  const configPath = useAppStore((s) => s.configPath);
  const configVersion = useAppStore((s) => s.configVersion);
  const plugins = useAppStore((s) => s.plugins);
  const isOfflineMode = useAppStore((s) => s.isOfflineMode);
  const offlineFileName = useAppStore((s) => s.offlineFileName);
  const exitOfflineMode = useAppStore((s) => s.exitOfflineMode);

  const yamlEditorRef = useRef<YamlEditorHandle>(null);
  const [originalConfig, setOriginalConfig] = useState(
    configEditorBaseline ?? yamlConfig,
  );
  const [saveStatus, setSaveStatus] = useState<"idle" | "success" | "error">(
    "idle",
  );
  const [copied, setCopied] = useState(false);
  const [isMac, setIsMac] = useState(false);
  const isMobile = useIsMobile();
  // On mobile the index panel is collapsed into a drawer so the editor is not
  // squeezed off-screen by the fixed-width side panel.
  const [indexOpen, setIndexOpen] = useState(false);

  const hasChanges = yamlConfig !== originalConfig;
  const modKey = isMac ? "⌘" : "Ctrl";

  useEffect(() => {
    const mac =
      typeof navigator !== "undefined" &&
      /mac|iphone|ipad|ipod/i.test(navigator.platform || navigator.userAgent);
    const timer = window.setTimeout(() => setIsMac(mac), 0);
    return () => window.clearTimeout(timer);
  }, []);

  // Config is loaded once on connect by the console layout. Do NOT reload
  // here on mount — switching into editor mode must not refetch and clobber
  // local state (apply-failed status, runningVersion, unsaved edits).

  // Resync the dirty-tracking baseline only when the persisted config
  // changes (after load or save) — NOT on every keystroke, otherwise
  // hasChanges collapses immediately and Save stays disabled.
  useEffect(() => {
    if (!configVersion || configEditorBaseline !== null) return;
    const timer = window.setTimeout(
      () => setOriginalConfig(useAppStore.getState().configText),
      0,
    );
    return () => window.clearTimeout(timer);
  }, [configEditorBaseline, configVersion]);

  const handleSave = async () => {
    setSaveStatus("idle");
    try {
      await saveConfig();
      setOriginalConfig(useAppStore.getState().configText);
      setSaveStatus("success");
      setTimeout(() => setSaveStatus("idle"), 3000);
    } catch {
      setSaveStatus("error");
    }
  };

  const handleReset = () => {
    setYamlConfig(originalConfig);
    setSaveStatus("idle");
  };

  const handleDownload = () => {
    if (!yamlConfig) return;
    const raw = (offlineFileName ?? "config.yaml").trim() || "config.yaml";
    const name = /\.(ya?ml)$/i.test(raw) ? raw : `${raw}.yaml`;
    const url = URL.createObjectURL(
      new Blob([yamlConfig], { type: "text/yaml" }),
    );
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = name;
    anchor.click();
    setTimeout(() => URL.revokeObjectURL(url), 0);
  };

  const handleCopy = () => {
    void navigator.clipboard
      .writeText(yamlConfig)
      .then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => {
        setSaveStatus("error");
      });
  };

  const handleJumpToLine = (line: number) => {
    yamlEditorRef.current?.jumpToLine(line);
    setIndexOpen(false);
  };

  const busy = isConfigSaving || isRestarting;

  const shortcutsBlock = (
    <div className="border-t px-3 py-3 flex-shrink-0 space-y-2">
      <p className="text-xs text-muted-foreground font-medium mb-1.5">
        {t(WEBUI.configEditor.shortcuts)}
      </p>
      <div className="flex items-center justify-between text-xs">
        <span className="text-muted-foreground">
          {t(WEBUI.configEditor.indent)}
        </span>
        <kbd className="px-1.5 py-0.5 bg-muted rounded font-mono text-xs">
          Tab
        </kbd>
      </div>
      <div className="flex items-center justify-between text-xs">
        <span className="text-muted-foreground">
          {t(WEBUI.configEditor.save)}
        </span>
        <div className="flex gap-0.5">
          <kbd className="px-1.5 py-0.5 bg-muted rounded font-mono text-xs">
            {modKey}
          </kbd>
          <kbd className="px-1.5 py-0.5 bg-muted rounded font-mono text-xs">
            S
          </kbd>
        </div>
      </div>
      <div className="flex items-center justify-between text-xs">
        <span className="text-muted-foreground">
          {t(WEBUI.configEditor.undo)}
        </span>
        <div className="flex gap-0.5">
          <kbd className="px-1.5 py-0.5 bg-muted rounded font-mono text-xs">
            {modKey}
          </kbd>
          <kbd className="px-1.5 py-0.5 bg-muted rounded font-mono text-xs">
            Z
          </kbd>
        </div>
      </div>
    </div>
  );

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between px-4 sm:px-6 py-3 sm:py-4 border-b bg-card/50">
        <div className="flex items-center gap-2 sm:gap-3 min-w-0 flex-wrap">
          <FileCode2 className="h-5 w-5 text-muted-foreground flex-shrink-0" />
          <div className="min-w-0">
            <h2 className="text-lg font-semibold">
              {t(WEBUI.configEditor.title)}
            </h2>
            <p className="text-sm text-muted-foreground truncate">
              {configPath}
            </p>
          </div>
          {isOfflineMode && (
            <Badge
              variant="outline"
              className="bg-yellow-500/10 text-yellow-600 dark:text-yellow-400 border-yellow-500/30"
            >
              {t(WEBUI.configEditor.offlineMode)}
            </Badge>
          )}
          {copied && (
            <Badge
              variant="outline"
              className="bg-primary/10 text-primary border-primary/30"
            >
              <ClipboardCheck className="h-3 w-3 mr-1" />
              {t(WEBUI.configEditor.copied)}
            </Badge>
          )}
          {hasChanges && (
            <Badge
              variant="outline"
              className="bg-yellow-500/10 text-yellow-600 dark:text-yellow-400 border-yellow-500/30"
            >
              {t(WEBUI.configEditor.unsaved)}
            </Badge>
          )}
          {saveStatus === "success" && (
            <Badge
              variant="outline"
              className="bg-primary/10 text-primary border-primary/30"
            >
              <CheckCircle2 className="h-3 w-3 mr-1" />
              {t(WEBUI.configEditor.saved)}
            </Badge>
          )}
          {saveStatus === "error" && (
            <Badge variant="destructive">
              <AlertCircle className="h-3 w-3 mr-1" />
              {t(WEBUI.configEditor.saveFailed)}
            </Badge>
          )}
          {configError && (
            <Badge variant="destructive" className="max-w-md truncate">
              <AlertCircle className="h-3 w-3 mr-1" />
              {configError}
            </Badge>
          )}
        </div>
        <div className="flex items-center gap-2 flex-wrap">
          {isMobile && (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setIndexOpen(true)}
            >
              <PanelRightOpen className="h-4 w-4 mr-1.5" />
              {t(WEBUI.configEditor.index)}
            </Button>
          )}
          <Button
            variant="outline"
            size="sm"
            onClick={handleReset}
            disabled={!hasChanges || busy}
          >
            <RotateCcw className="h-4 w-4 mr-1.5" />
            {t(WEBUI.configEditor.reset)}
          </Button>
          {isOfflineMode ? (
            <>
              <Button
                variant="outline"
                size="sm"
                onClick={handleDownload}
                disabled={!yamlConfig}
              >
                <Download className="h-4 w-4 mr-1.5" />
                {t(WEBUI.configEditor.download)}
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={handleCopy}
                disabled={!yamlConfig}
              >
                <Copy className="h-4 w-4 mr-1.5" />
                {t(WEBUI.configEditor.copy)}
              </Button>
              <Button variant="ghost" size="sm" onClick={exitOfflineMode}>
                <LogOut className="h-4 w-4 mr-1.5" />
                {t(WEBUI.configEditor.exitOffline)}
              </Button>
            </>
          ) : (
            <Button
              variant="outline"
              size="sm"
              onClick={handleSave}
              disabled={!hasChanges || busy || Boolean(configError)}
            >
              {isConfigSaving ? (
                <Spinner className="h-4 w-4 mr-1.5" />
              ) : (
                <Save className="h-4 w-4 mr-1.5" />
              )}
              {t(WEBUI.configEditor.save)}
            </Button>
          )}
        </div>
      </div>

      <div className="flex-1 min-h-0 p-3 sm:p-6 flex flex-col">
        <div className="flex-1 min-h-0 flex gap-6">
          <div className="flex-1 min-w-0 min-h-0">
            <YamlEditor
              ref={yamlEditorRef}
              value={yamlConfig}
              onChange={setYamlConfig}
              onSave={() => {
                if (isOfflineMode) {
                  handleDownload();
                  return;
                }
                if (hasChanges && !busy && !configError) void handleSave();
              }}
              className="h-full"
              readOnly={isConfigLoading || busy}
              variant="config"
              backendValidation={!isOfflineMode}
              plugins={plugins}
            />
          </div>

          {!isMobile && (
            <Card className="w-80 flex-shrink-0 flex flex-col min-h-0">
              <CardHeader className="flex-shrink-0 pb-2">
                <CardTitle className="text-sm">
                  {t(WEBUI.configEditor.pluginIndexTitle)}
                </CardTitle>
                <CardDescription>
                  {t(WEBUI.configEditor.jumpToDefinition)}
                </CardDescription>
              </CardHeader>
              <CardContent className="flex-1 min-h-0 overflow-y-auto pb-2 px-3">
                <PluginIndexPanel
                  yamlText={yamlConfig}
                  onJumpToLine={handleJumpToLine}
                />
              </CardContent>
              {shortcutsBlock}
            </Card>
          )}
        </div>
      </div>

      {isMobile && (
        <Sheet open={indexOpen} onOpenChange={setIndexOpen}>
          {/* Width is left to SheetContent's default (data-[side=right]:w-3/4,
              i.e. 75vw). A plain w-* override here would lose to that
              variant-prefixed base class under tailwind-merge anyway, and 75vw
              already leaves a comfortable backdrop strip to tap-close on mobile. */}
          <SheetContent side="right" className="gap-0 p-0">
            <SheetHeader className="px-4 pt-4 pb-2">
              <SheetTitle className="text-sm">
                {t(WEBUI.configEditor.pluginIndexTitle)}
              </SheetTitle>
              <SheetDescription>
                {t(WEBUI.configEditor.jumpToDefinition)}
              </SheetDescription>
            </SheetHeader>
            <div className="flex-1 min-h-0 overflow-y-auto px-3 pb-2">
              <PluginIndexPanel
                yamlText={yamlConfig}
                onJumpToLine={handleJumpToLine}
              />
            </div>
            {shortcutsBlock}
          </SheetContent>
        </Sheet>
      )}
    </div>
  );
}
