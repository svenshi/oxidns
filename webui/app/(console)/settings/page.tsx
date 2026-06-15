"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { AppHeader } from "@/components/shell/app-header";
import { useAppStore } from "@/lib/store";
import { isSameServerIdentity, useAuthStore } from "@/lib/auth-store";
import {
  useUpdateStore,
  DEFAULT_UPGRADE_CONFIG,
  type UpgradeBundle,
} from "@/lib/update-store";
import { stringifyOxiDnsConfig, type OxiDnsConfig } from "@/lib/oxidns-config";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import {
  ArrowUpCircle,
  CheckCircle2,
  CircleAlert,
  Copy,
  Cpu,
  FileCode2,
  Globe,
  PlugZap,
  RefreshCw,
  ScrollText,
  Server,
  ShieldCheck,
  SlidersHorizontal,
} from "lucide-react";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

export default function SettingsPage() {
  const { t } = useI18n();
  const router = useRouter();
  const serverConfig = useAuthStore((s) => s.serverConfig);
  const setServerConfig = useAuthStore((s) => s.setServerConfig);
  const connect = useAuthStore((s) => s.connect);
  const isConnected = useAuthStore((s) => s.isConnected);
  const isConnecting = useAuthStore((s) => s.isConnecting);
  const connectionError = useAuthStore((s) => s.connectionError);

  const upgradeConfig = useUpdateStore((s) => s.upgradeConfig);
  const setUpgradeConfig = useUpdateStore((s) => s.setUpgradeConfig);
  const updateInfo = useUpdateStore((s) => s.updateInfo);
  const isChecking = useUpdateStore((s) => s.isChecking);
  const isApplying = useUpdateStore((s) => s.isApplying);
  const lastCheckedAt = useUpdateStore((s) => s.lastCheckedAt);
  const checkError = useUpdateStore((s) => s.checkError);
  const applyError = useUpdateStore((s) => s.applyError);
  const checkForUpdates = useUpdateStore((s) => s.checkForUpdates);
  const triggerUpgrade = useUpdateStore((s) => s.triggerUpgrade);
  const [copiedCmd, setCopiedCmd] = useState(false);
  const [tokenPersistenceHelpOpen, setTokenPersistenceHelpOpen] =
    useState(false);

  const configModel = useAppStore((s) => s.configModel);
  const configPath = useAppStore((s) => s.configPath);
  const configVersion = useAppStore((s) => s.configVersion);
  const configError = useAppStore((s) => s.configError);
  const dependencyGraph = useAppStore((s) => s.dependencyGraph);
  const health = useAppStore((s) => s.health);
  const system = useAppStore((s) => s.system);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const reloadStatus = useAppStore((s) => s.reloadStatus);
  const setYamlConfig = useAppStore((s) => s.setYamlConfig);
  const saveConfig = useAppStore((s) => s.saveConfig);
  const loadConfig = useAppStore((s) => s.loadConfig);
  const resetBackendSession = useAppStore((s) => s.resetBackendSession);
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const isRestarting = useAppStore((s) => s.isRestarting);
  const restartApp = useAppStore((s) => s.restartApp);
  const webUiMode = useAppStore((s) => s.webUiMode);
  const setWebUiMode = useAppStore((s) => s.setWebUiMode);

  const [backendUrl, setBackendUrl] = useState(serverConfig.url);
  const [workerThreads, setWorkerThreads] = useState("");
  const [apiListen, setApiListen] = useState("");
  const [apiSslEnabled, setApiSslEnabled] = useState(false);
  const [apiSslCert, setApiSslCert] = useState("");
  const [apiSslKey, setApiSslKey] = useState("");
  const [apiSslClientCa, setApiSslClientCa] = useState("");
  const [apiSslRequireClientCert, setApiSslRequireClientCert] = useState(false);
  const [apiAuthEnabled, setApiAuthEnabled] = useState(false);
  const [apiAuthUsername, setApiAuthUsername] = useState("");
  const [apiAuthPassword, setApiAuthPassword] = useState("");
  // Account & Security card local form state
  const [authEditMode, setAuthEditMode] = useState<
    null | "enable" | "change" | "disable"
  >(null);
  const [newAuthUsername, setNewAuthUsername] = useState("");
  const [newAuthPassword, setNewAuthPassword] = useState("");
  const [confirmAuthPassword, setConfirmAuthPassword] = useState("");
  const [apiCorsOrigins, setApiCorsOrigins] = useState("");
  const [apiWebuiEnabled, setApiWebuiEnabled] = useState(false);
  const [apiWebuiRoot, setApiWebuiRoot] = useState("");
  const [apiWebuiIndex, setApiWebuiIndex] = useState("");
  const [logLevel, setLogLevel] = useState("info");
  const [logFile, setLogFile] = useState("");
  const [rotationType, setRotationType] = useState("never");
  const [maxFiles, setMaxFiles] = useState("");

  useEffect(() => {
    const timer = window.setTimeout(() => {
      const runtime = asRecord(configModel.runtime);
      const api = asRecord(configModel.api);
      const http = api.http;
      const httpObj =
        typeof http === "string" ? { listen: http } : asRecord(http);
      const ssl = asRecord(httpObj.ssl);
      const auth = asRecord(httpObj.auth);
      const cors = asRecord(httpObj.cors);
      const webui = asRecord(httpObj.webui);
      const log = asRecord(configModel.log);
      const rotation = asRecord(log.rotation);

      setWorkerThreads(String(runtime.worker_threads ?? ""));
      setApiListen(String(httpObj.listen ?? ""));
      setApiSslEnabled(Boolean(ssl.cert || ssl.key));
      setApiSslCert(String(ssl.cert ?? ""));
      setApiSslKey(String(ssl.key ?? ""));
      setApiSslClientCa(String(ssl.client_ca ?? ""));
      setApiSslRequireClientCert(Boolean(ssl.require_client_cert ?? false));
      setApiAuthEnabled(auth.type === "basic");
      setApiAuthUsername(String(auth.username ?? ""));
      setApiAuthPassword(String(auth.password ?? ""));
      const origins = Array.isArray(cors.allowed_origins)
        ? (cors.allowed_origins as string[])
        : [];
      setApiCorsOrigins(origins.join("\n"));
      setApiWebuiEnabled(Boolean(webui.root));
      setApiWebuiRoot(String(webui.root ?? ""));
      setApiWebuiIndex(String(webui.index ?? ""));
      setLogLevel(String(log.level ?? "info"));
      setLogFile(String(log.file ?? ""));
      setRotationType(String(rotation.type ?? "never"));
      setMaxFiles(rotation.max_files != null ? String(rotation.max_files) : "");
    }, 0);
    return () => window.clearTimeout(timer);
  }, [configModel]);

  const canConnect = backendUrl.trim().length > 0;
  const runtimeVersion = system?.build
    ? `${system.build.version} (${system.build.bundle})`
    : health?.build_bundle
      ? `${health.version} (${health.build_bundle})`
      : (system?.version ?? health?.version ?? "-");

  const applyServerConfig = (nextConfig: typeof serverConfig) => {
    const backendChanged = !isSameServerIdentity(serverConfig, nextConfig);
    setServerConfig(nextConfig);
    if (backendChanged) resetBackendSession();
  };

  const handleSaveConnection = () => {
    applyServerConfig({ ...serverConfig, url: backendUrl.trim() });
  };

  const runtimeVersionForCheck = system?.build
    ? `${system.build.version}`
    : (system?.version ?? health?.version ?? "");

  // null = build info not yet loaded; true/false = feature presence known
  const backendSupportsUpgrade =
    buildInfo != null
      ? buildInfo.enabled_features.includes("plugin-upgrade")
      : null;

  const handleCheckUpdates = () => {
    if (runtimeVersionForCheck) {
      void checkForUpdates(runtimeVersionForCheck);
    }
  };

  const buildUpgradeCliCommand = () => {
    const parts = ["oxidns", "upgrade", "apply"];
    if (upgradeConfig.repository !== DEFAULT_UPGRADE_CONFIG.repository) {
      parts.push("--repository", upgradeConfig.repository);
    }
    if (upgradeConfig.bundle !== "auto") {
      parts.push("--bundle", upgradeConfig.bundle);
    }
    if (upgradeConfig.socks5.trim()) {
      parts.push("--socks5", upgradeConfig.socks5.trim());
    }
    if (upgradeConfig.githubToken.trim()) {
      parts.push("--github-token", "<GITHUB_TOKEN>");
    }
    if (upgradeConfig.allowPrerelease) {
      parts.push("--allow-prerelease");
    }
    return parts.join(" ");
  };

  const handleCopyCommand = async () => {
    try {
      await navigator.clipboard.writeText(buildUpgradeCliCommand());
      setCopiedCmd(true);
      setTimeout(() => setCopiedCmd(false), 2000);
    } catch {
      // ignore
    }
  };

  const handleConnect = async () => {
    const nextConfig = { ...serverConfig, url: backendUrl.trim() };
    applyServerConfig(nextConfig);
    const ok = await connect(nextConfig);
    if (ok) await loadConfig();
  };

  const enterStandardMode = () => {
    if (webUiMode === "standard") return;
    setWebUiMode("standard", { dismissSelection: true });
    router.push("/standard");
  };

  const enterExpertMode = () => {
    if (webUiMode === "expert") return;
    setWebUiMode("expert", { dismissSelection: true });
    router.push("/");
  };

  type AuthOverride = { enabled: boolean; username: string; password: string };

  const buildApiHttpConfig = (authOverride?: AuthOverride): unknown => {
    const authEnabled =
      authOverride !== undefined ? authOverride.enabled : apiAuthEnabled;
    const authUsername =
      authOverride !== undefined ? authOverride.username : apiAuthUsername;
    const authPassword =
      authOverride !== undefined ? authOverride.password : apiAuthPassword;

    const sslConfig =
      apiSslEnabled && apiSslCert.trim() && apiSslKey.trim()
        ? {
            cert: apiSslCert.trim(),
            key: apiSslKey.trim(),
            ...(apiSslClientCa.trim()
              ? { client_ca: apiSslClientCa.trim() }
              : {}),
            ...(apiSslRequireClientCert ? { require_client_cert: true } : {}),
          }
        : undefined;
    const authConfig =
      authEnabled && authUsername.trim()
        ? {
            type: "basic",
            username: authUsername.trim(),
            password: authPassword,
          }
        : undefined;
    const corsOriginsList = apiCorsOrigins
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean);
    const corsConfig =
      corsOriginsList.length > 0
        ? { allowed_origins: corsOriginsList }
        : undefined;
    const webuiConfig =
      apiWebuiEnabled && apiWebuiRoot.trim()
        ? {
            root: apiWebuiRoot.trim(),
            ...(apiWebuiIndex.trim() ? { index: apiWebuiIndex.trim() } : {}),
          }
        : undefined;
    const hasDetail = sslConfig || authConfig || corsConfig || webuiConfig;
    if (!hasDetail) return apiListen.trim();
    return {
      listen: apiListen.trim(),
      ...(sslConfig ? { ssl: sslConfig } : {}),
      ...(authConfig ? { auth: authConfig } : {}),
      ...(corsConfig ? { cors: corsConfig } : {}),
      ...(webuiConfig ? { webui: webuiConfig } : {}),
    };
  };

  const buildTopLevelConfig = (authOverride?: AuthOverride): OxiDnsConfig => {
    const nextRuntime: Record<string, unknown> = {
      ...asRecord(configModel.runtime),
    };
    if (workerThreads.trim()) {
      nextRuntime.worker_threads = Number(workerThreads);
    } else {
      delete nextRuntime.worker_threads;
    }

    const nextApi: Record<string, unknown> = { ...asRecord(configModel.api) };
    if (apiListen.trim()) {
      nextApi.http = buildApiHttpConfig(authOverride);
    } else {
      delete nextApi.http;
    }

    return {
      ...configModel,
      runtime: Object.keys(nextRuntime).length > 0 ? nextRuntime : undefined,
      api: Object.keys(nextApi).length > 0 ? nextApi : undefined,
      log: {
        ...asRecord(configModel.log),
        level: logLevel,
        ...(logFile.trim() ? { file: logFile.trim() } : { file: undefined }),
        rotation:
          rotationType === "never"
            ? { type: "never" }
            : {
                type: rotationType,
                ...(maxFiles.trim() !== ""
                  ? { max_files: Number(maxFiles) }
                  : {}),
              },
      },
    };
  };

  const handleSaveTopLevelConfig = async () => {
    setYamlConfig(stringifyOxiDnsConfig(buildTopLevelConfig()));
    await saveConfig();
  };

  const handleRestartTopLevelConfig = async () => {
    setYamlConfig(stringifyOxiDnsConfig(buildTopLevelConfig()));
    await restartApp();
  };

  // Dedicated auth save: updates config.yaml + syncs WebUI connection credentials atomically.
  const handleAuthSave = async (
    enabled: boolean,
    uname: string,
    pwd: string,
  ) => {
    const override: AuthOverride = { enabled, username: uname, password: pwd };
    setYamlConfig(stringifyOxiDnsConfig(buildTopLevelConfig(override)));

    if (enabled && uname.trim()) {
      applyServerConfig({
        ...serverConfig,
        requiresAuth: true,
        username: uname.trim(),
        password: pwd,
      });
    } else {
      applyServerConfig({
        ...serverConfig,
        requiresAuth: false,
        username: "",
        password: "",
      });
    }

    setApiAuthEnabled(enabled);
    setApiAuthUsername(enabled ? uname : "");
    setApiAuthPassword(enabled ? pwd : "");
    setAuthEditMode(null);
    setNewAuthPassword("");
    setConfirmAuthPassword("");
    await restartApp();
  };

  return (
    <>
      <AppHeader title={t(WEBUI.shell.settings)} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="max-w-4xl space-y-6">
          <Card>
            <CardHeader>
              <div className="flex items-start justify-between gap-3">
                <div>
                  <CardTitle className="flex items-center gap-2">
                    <PlugZap className="h-5 w-5" />
                    {t(WEBUI.settings.backendCard)}
                  </CardTitle>
                  <CardDescription className="mt-1.5">
                    {t(WEBUI.settings.backendCardDesc)}
                  </CardDescription>
                </div>
                <Badge
                  variant="outline"
                  className={
                    isConnected
                      ? "bg-primary/10 text-primary border-primary/30"
                      : "bg-muted text-muted-foreground"
                  }
                >
                  {isConnected
                    ? t(WEBUI.settings.connected)
                    : t(WEBUI.settings.disconnected)}
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-4">
              <Field>
                <FieldLabel>{t(WEBUI.settings.serviceUrl)}</FieldLabel>
                <Input
                  value={backendUrl}
                  onChange={(event) => setBackendUrl(event.target.value)}
                  placeholder={t(WEBUI.settings.serviceUrlPlaceholder)}
                  className="font-mono"
                />
              </Field>
              {connectionError && (
                <div className="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                  <CircleAlert className="h-4 w-4" />
                  {connectionError}
                </div>
              )}
              <div className="flex flex-wrap items-center gap-2">
                <Button onClick={handleSaveConnection}>
                  {t(WEBUI.settings.saveAddress)}
                </Button>
                <Button
                  variant="outline"
                  onClick={handleConnect}
                  disabled={!canConnect || isConnecting}
                >
                  <PlugZap className="h-4 w-4 mr-1.5" />
                  {isConnecting
                    ? t(WEBUI.settings.connecting)
                    : t(WEBUI.settings.reconnect)}
                </Button>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <div className="flex items-start justify-between gap-3">
                <div>
                  <CardTitle className="flex items-center gap-2">
                    <SlidersHorizontal className="h-5 w-5" />
                    工作模式
                  </CardTitle>
                  <CardDescription className="mt-1.5">
                    标准模式使用表单读写 YAML；专家模式保留完整插件中心和 YAML
                    控制台。
                  </CardDescription>
                </div>
                <Badge variant="secondary">
                  {webUiMode === "standard" ? "标准模式" : "专家模式"}
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="grid gap-3 md:grid-cols-2">
              <div className="flex flex-col justify-between gap-4 rounded-lg border p-4">
                <div className="space-y-1.5">
                  <div className="flex items-center gap-2 font-medium">
                    <SlidersHorizontal className="h-4 w-4" />
                    标准模式
                  </div>
                  <p className="text-sm leading-6 text-muted-foreground">
                    适合日常 DNS 管理，通过开关和输入框生成标准配置。
                  </p>
                </div>
                <Button
                  variant={webUiMode === "standard" ? "secondary" : "default"}
                  onClick={enterStandardMode}
                  disabled={webUiMode === "standard"}
                >
                  {webUiMode === "standard" ? "当前模式" : "切换到标准模式"}
                </Button>
              </div>
              <div className="flex flex-col justify-between gap-4 rounded-lg border p-4">
                <div className="space-y-1.5">
                  <div className="flex items-center gap-2 font-medium">
                    <FileCode2 className="h-4 w-4" />
                    专家模式
                  </div>
                  <p className="text-sm leading-6 text-muted-foreground">
                    适合直接管理插件、拓扑、历史版本和完整 YAML 配置。
                  </p>
                </div>
                <Button
                  variant={webUiMode === "expert" ? "secondary" : "outline"}
                  onClick={enterExpertMode}
                  disabled={webUiMode === "expert"}
                >
                  {webUiMode === "expert" ? "当前模式" : "切换到专家模式"}
                </Button>
              </div>
            </CardContent>
          </Card>

          {isConnected && (
            <>
              <Card>
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <ShieldCheck className="h-5 w-5" />
                    {t(WEBUI.settings.accountCard)}
                  </CardTitle>
                  <CardDescription>
                    {t(WEBUI.settings.accountCardDesc)}
                  </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                  {authEditMode === null && (
                    <>
                      {apiAuthEnabled ? (
                        <div className="flex flex-wrap items-center justify-between gap-3">
                          <div className="flex items-center gap-2">
                            <Badge
                              variant="outline"
                              className="bg-primary/10 text-primary border-primary/30"
                            >
                              {t(WEBUI.settings.authBadgeEnabled)}
                            </Badge>
                            <span className="text-sm text-muted-foreground">
                              {t(WEBUI.settings.accountPrefix)}
                              <span className="font-mono font-medium text-foreground">
                                {apiAuthUsername}
                              </span>
                            </span>
                          </div>
                          <div className="flex flex-wrap gap-2">
                            <Button
                              variant="outline"
                              size="sm"
                              onClick={() => {
                                setNewAuthUsername(apiAuthUsername);
                                setNewAuthPassword("");
                                setConfirmAuthPassword("");
                                setAuthEditMode("change");
                              }}
                              disabled={isRestarting}
                            >
                              {t(WEBUI.settings.changePassword)}
                            </Button>
                            <Button
                              variant="outline"
                              size="sm"
                              onClick={() => setAuthEditMode("disable")}
                              disabled={isRestarting}
                            >
                              {t(WEBUI.settings.disableAuth)}
                            </Button>
                          </div>
                        </div>
                      ) : (
                        <div className="space-y-3">
                          <div className="flex items-center gap-2 rounded-lg border border-yellow-500/30 bg-yellow-500/10 px-3 py-2 text-sm text-yellow-700 dark:text-yellow-400">
                            <CircleAlert className="h-4 w-4 shrink-0" />
                            {t(WEBUI.settings.noAuthWarning)}
                          </div>
                          <Button
                            size="sm"
                            onClick={() => {
                              setNewAuthUsername("");
                              setNewAuthPassword("");
                              setConfirmAuthPassword("");
                              setAuthEditMode("enable");
                            }}
                            disabled={isRestarting}
                          >
                            {t(WEBUI.settings.setAccountPassword)}
                          </Button>
                        </div>
                      )}
                    </>
                  )}

                  {(authEditMode === "enable" || authEditMode === "change") && (
                    <form
                      onSubmit={(e) => {
                        e.preventDefault();
                        void handleAuthSave(
                          true,
                          newAuthUsername,
                          newAuthPassword,
                        );
                      }}
                      className="space-y-4"
                    >
                      <Field>
                        <FieldLabel>
                          {t(WEBUI.settings.usernameLabel)}
                        </FieldLabel>
                        <Input
                          value={newAuthUsername}
                          onChange={(e) => setNewAuthUsername(e.target.value)}
                          autoComplete="username"
                          autoFocus
                          className="max-w-xs"
                        />
                      </Field>
                      <div className="grid gap-4 sm:grid-cols-2">
                        <Field>
                          <FieldLabel>
                            {authEditMode === "change"
                              ? t(WEBUI.settings.newPasswordLabel)
                              : t(WEBUI.settings.passwordLabel)}
                          </FieldLabel>
                          <Input
                            type="password"
                            value={newAuthPassword}
                            onChange={(e) => setNewAuthPassword(e.target.value)}
                            autoComplete="new-password"
                          />
                        </Field>
                        <Field>
                          <FieldLabel>
                            {t(WEBUI.settings.confirmPasswordLabel)}
                          </FieldLabel>
                          <Input
                            type="password"
                            value={confirmAuthPassword}
                            onChange={(e) =>
                              setConfirmAuthPassword(e.target.value)
                            }
                            autoComplete="new-password"
                          />
                        </Field>
                      </div>
                      {confirmAuthPassword.length > 0 &&
                        newAuthPassword !== confirmAuthPassword && (
                          <p className="text-sm text-destructive">
                            {t(WEBUI.settings.passwordMismatch)}
                          </p>
                        )}
                      <div className="flex flex-wrap gap-2">
                        <Button
                          type="submit"
                          disabled={
                            isRestarting ||
                            !newAuthUsername.trim() ||
                            !newAuthPassword ||
                            newAuthPassword !== confirmAuthPassword
                          }
                        >
                          <RefreshCw className="h-4 w-4 mr-1.5" />
                          {isRestarting
                            ? t(WEBUI.settings.restarting)
                            : t(WEBUI.settings.saveAndRestart)}
                        </Button>
                        <Button
                          type="button"
                          variant="outline"
                          onClick={() => setAuthEditMode(null)}
                          disabled={isRestarting}
                        >
                          {t(WEBUI.common.cancel)}
                        </Button>
                      </div>
                    </form>
                  )}

                  {authEditMode === "disable" && (
                    <div className="space-y-4">
                      <div className="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                        <CircleAlert className="h-4 w-4 shrink-0" />
                        {t(WEBUI.settings.disableAuthWarning)}
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <Button
                          variant="destructive"
                          onClick={() => void handleAuthSave(false, "", "")}
                          disabled={isRestarting}
                        >
                          <RefreshCw className="h-4 w-4 mr-1.5" />
                          {isRestarting
                            ? t(WEBUI.settings.restarting)
                            : t(WEBUI.settings.confirmDisableRestart)}
                        </Button>
                        <Button
                          variant="outline"
                          onClick={() => setAuthEditMode(null)}
                          disabled={isRestarting}
                        >
                          {t(WEBUI.common.cancel)}
                        </Button>
                      </div>
                    </div>
                  )}
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <Server className="h-5 w-5" />
                    {t(WEBUI.settings.runtimeStatusCard)}
                  </CardTitle>
                  <CardDescription>
                    {t(WEBUI.settings.runtimeStatusDesc)}
                  </CardDescription>
                </CardHeader>
                <CardContent className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                  <InfoTile
                    label={t(WEBUI.settings.compiledVersion)}
                    value={runtimeVersion}
                  />
                  <InfoTile
                    label={t(WEBUI.settings.platformLabel)}
                    value={system ? `${system.os}/${system.arch}` : "-"}
                  />
                  <InfoTile
                    label={t(WEBUI.settings.healthStatusLabel)}
                    value={health?.status ?? "-"}
                  />
                  <InfoTile
                    label={t(WEBUI.settings.reloadStatusLabel)}
                    value={reloadStatus?.status ?? "-"}
                  />
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <FileCode2 className="h-5 w-5" />
                    {t(WEBUI.settings.configSummaryCard)}
                  </CardTitle>
                </CardHeader>
                <CardContent className="grid gap-4 sm:grid-cols-2">
                  <InfoTile
                    label={t(WEBUI.settings.configFileLabel)}
                    value={configPath}
                  />
                  <InfoTile
                    label={t(WEBUI.settings.versionLabel)}
                    value={configVersion?.slice(0, 12) ?? "-"}
                  />
                  <InfoTile
                    label={t(WEBUI.settings.pluginCountLabel)}
                    value={String(
                      dependencyGraph?.nodes.length ??
                        configModel.plugins.length,
                    )}
                  />
                  <InfoTile
                    label={t(WEBUI.settings.initOrderLabel)}
                    value={String(dependencyGraph?.init_order.length ?? 0)}
                  />
                  <div className="sm:col-span-2">
                    <Badge
                      variant={configError ? "destructive" : "outline"}
                      className={
                        configError ? "" : "bg-primary/10 text-primary"
                      }
                    >
                      {configError ? (
                        <CircleAlert className="h-3 w-3 mr-1" />
                      ) : (
                        <CheckCircle2 className="h-3 w-3 mr-1" />
                      )}
                      {configError ?? t(WEBUI.settings.configOkBadge)}
                    </Badge>
                  </div>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <Cpu className="h-5 w-5" />
                    {t(WEBUI.settings.runtimeCard)}
                  </CardTitle>
                  <CardDescription>
                    {t(WEBUI.settings.runtimeCardDesc)}
                  </CardDescription>
                </CardHeader>
                <CardContent className="space-y-5">
                  <Field>
                    <FieldLabel>{t(WEBUI.settings.workerThreads)}</FieldLabel>
                    <p className="text-xs text-muted-foreground mb-2">
                      {t(WEBUI.settings.workerThreadsDesc)}
                    </p>
                    <Input
                      value={workerThreads}
                      onChange={(event) => setWorkerThreads(event.target.value)}
                      type="number"
                      min={1}
                      placeholder={t(WEBUI.settings.workerThreadsPlaceholder)}
                      className="font-mono max-w-xs"
                    />
                  </Field>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      onClick={handleSaveTopLevelConfig}
                      disabled={isConfigSaving || isRestarting || !isConnected}
                    >
                      {t(WEBUI.common.saveConfig)}
                    </Button>
                    <Button
                      variant="outline"
                      onClick={handleRestartTopLevelConfig}
                      disabled={isConfigSaving || isRestarting || !isConnected}
                    >
                      <RefreshCw className="h-4 w-4 mr-1.5" />
                      {isRestarting
                        ? t(WEBUI.settings.restarting)
                        : t(WEBUI.settings.saveAndRestart)}
                    </Button>
                  </div>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <Globe className="h-5 w-5" />
                    {t(WEBUI.settings.mgmtApiCard)}
                  </CardTitle>
                  <CardDescription>
                    {t(WEBUI.settings.mgmtApiDesc)}
                  </CardDescription>
                </CardHeader>
                <CardContent className="space-y-6">
                  <div className="space-y-2">
                    <p className="text-sm font-medium">
                      {t(WEBUI.settings.listenSection)}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      {t(WEBUI.settings.listenDesc)}
                    </p>
                    <Input
                      value={apiListen}
                      onChange={(e) => setApiListen(e.target.value)}
                      placeholder=":9199"
                      className="font-mono"
                    />
                  </div>

                  <div className="space-y-4">
                    <div className="flex items-start justify-between gap-4">
                      <div>
                        <p className="text-sm font-medium">
                          {t(WEBUI.settings.tlsSection)}
                        </p>
                        <p className="text-xs text-muted-foreground mt-1">
                          {t(WEBUI.settings.tlsDesc)}
                        </p>
                      </div>
                      <Switch
                        checked={apiSslEnabled}
                        onCheckedChange={setApiSslEnabled}
                        aria-label={t(WEBUI.settings.enableTls)}
                      />
                    </div>
                    {apiSslEnabled && (
                      <div className="space-y-4">
                        <div className="grid gap-4 sm:grid-cols-2">
                          <Field>
                            <FieldLabel>
                              {t(WEBUI.settings.certPath)}
                            </FieldLabel>
                            <Input
                              value={apiSslCert}
                              onChange={(e) => setApiSslCert(e.target.value)}
                              placeholder="/etc/oxidns/api.crt"
                              className="font-mono"
                            />
                          </Field>
                          <Field>
                            <FieldLabel>{t(WEBUI.settings.keyPath)}</FieldLabel>
                            <Input
                              value={apiSslKey}
                              onChange={(e) => setApiSslKey(e.target.value)}
                              placeholder="/etc/oxidns/api.key"
                              className="font-mono"
                            />
                          </Field>
                          <Field>
                            <FieldLabel>
                              {t(WEBUI.settings.clientCa)}
                            </FieldLabel>
                            <p className="text-xs text-muted-foreground mb-2">
                              {t(WEBUI.settings.clientCaDesc)}
                            </p>
                            <Input
                              value={apiSslClientCa}
                              onChange={(e) =>
                                setApiSslClientCa(e.target.value)
                              }
                              placeholder="/etc/oxidns/client-ca.crt"
                              className="font-mono"
                            />
                          </Field>
                        </div>
                        <div className="flex items-start justify-between gap-4">
                          <div>
                            <p className="text-sm font-medium">
                              {t(WEBUI.settings.requireClientCert)}
                            </p>
                            <p className="text-xs text-muted-foreground mt-1">
                              {t(WEBUI.settings.requireClientCertDesc)}
                            </p>
                          </div>
                          <Switch
                            checked={apiSslRequireClientCert}
                            onCheckedChange={setApiSslRequireClientCert}
                            aria-label={t(WEBUI.settings.requireClientCert)}
                          />
                        </div>
                      </div>
                    )}
                  </div>

                  <div className="flex items-center justify-between gap-4">
                    <div>
                      <p className="text-sm font-medium">
                        {t(WEBUI.settings.authSection)}
                      </p>
                      <p className="text-xs text-muted-foreground mt-1">
                        {t(WEBUI.settings.authSectionDesc)}
                      </p>
                    </div>
                    <Badge
                      variant="outline"
                      className={
                        apiAuthEnabled
                          ? "bg-primary/10 text-primary border-primary/30"
                          : "text-muted-foreground"
                      }
                    >
                      {apiAuthEnabled
                        ? t(WEBUI.settings.authEnabledFor, {
                            username: apiAuthUsername,
                          })
                        : t(WEBUI.settings.authNotEnabled)}
                    </Badge>
                  </div>

                  <div className="space-y-2">
                    <p className="text-sm font-medium">
                      {t(WEBUI.settings.corsSection)}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      {t(WEBUI.settings.corsDesc)}
                    </p>
                    <Textarea
                      value={apiCorsOrigins}
                      onChange={(e) => setApiCorsOrigins(e.target.value)}
                      placeholder={
                        "http://localhost:3000\nhttps://console.example.com"
                      }
                      className="font-mono text-xs min-h-[80px] resize-y"
                      spellCheck={false}
                    />
                  </div>

                  <div className="space-y-4">
                    <div className="flex items-start justify-between gap-4">
                      <div>
                        <p className="text-sm font-medium">
                          {t(WEBUI.settings.webuiSection)}
                        </p>
                        <p className="text-xs text-muted-foreground mt-1">
                          {t(WEBUI.settings.webuiDesc)}
                        </p>
                      </div>
                      <Switch
                        checked={apiWebuiEnabled}
                        onCheckedChange={setApiWebuiEnabled}
                        aria-label={t(WEBUI.settings.mountWebui)}
                      />
                    </div>
                    {apiWebuiEnabled && (
                      <div className="grid gap-4 sm:grid-cols-2">
                        <Field>
                          <FieldLabel>
                            {t(WEBUI.settings.staticRoot)}
                          </FieldLabel>
                          <Input
                            value={apiWebuiRoot}
                            onChange={(e) => setApiWebuiRoot(e.target.value)}
                            placeholder="/etc/oxidns/webui"
                            className="font-mono"
                          />
                        </Field>
                        <Field>
                          <FieldLabel>{t(WEBUI.settings.indexFile)}</FieldLabel>
                          <Input
                            value={apiWebuiIndex}
                            onChange={(e) => setApiWebuiIndex(e.target.value)}
                            placeholder="index.html"
                            className="font-mono"
                          />
                        </Field>
                      </div>
                    )}
                  </div>

                  <div className="flex flex-wrap gap-2">
                    <Button
                      onClick={handleSaveTopLevelConfig}
                      disabled={isConfigSaving || isRestarting || !isConnected}
                    >
                      {t(WEBUI.common.saveConfig)}
                    </Button>
                    <Button
                      variant="outline"
                      onClick={handleRestartTopLevelConfig}
                      disabled={isConfigSaving || isRestarting || !isConnected}
                    >
                      <RefreshCw className="h-4 w-4 mr-1.5" />
                      {isRestarting
                        ? t(WEBUI.settings.restarting)
                        : t(WEBUI.settings.saveAndRestart)}
                    </Button>
                  </div>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <ScrollText className="h-5 w-5" />
                    {t(WEBUI.settings.logCard)}
                  </CardTitle>
                  <CardDescription>
                    {t(WEBUI.settings.logCardDesc)}
                  </CardDescription>
                </CardHeader>
                <CardContent className="space-y-5">
                  <div className="grid gap-4 sm:grid-cols-2">
                    <Field>
                      <FieldLabel>{t(WEBUI.settings.logLevelLabel)}</FieldLabel>
                      <p className="text-xs text-muted-foreground mb-2">
                        {t(WEBUI.settings.logLevelDesc)}
                      </p>
                      <Select value={logLevel} onValueChange={setLogLevel}>
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {[
                            "trace",
                            "debug",
                            "info",
                            "warn",
                            "error",
                            "off",
                          ].map((level) => (
                            <SelectItem key={level} value={level}>
                              {level}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </Field>
                    <Field>
                      <FieldLabel>{t(WEBUI.settings.logFilePath)}</FieldLabel>
                      <p className="text-xs text-muted-foreground mb-2">
                        {t(WEBUI.settings.logFileDesc)}
                      </p>
                      <Input
                        value={logFile}
                        onChange={(event) => setLogFile(event.target.value)}
                        placeholder={t(WEBUI.settings.logFilePlaceholder)}
                        className="font-mono"
                      />
                    </Field>
                    <Field>
                      <FieldLabel>{t(WEBUI.settings.logRotation)}</FieldLabel>
                      <p className="text-xs text-muted-foreground mb-2">
                        {t(WEBUI.settings.logRotationDesc)}
                      </p>
                      <Select
                        value={rotationType}
                        onValueChange={setRotationType}
                      >
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="never">
                            {t(WEBUI.settings.rotationNever)}
                          </SelectItem>
                          <SelectItem value="minutely">
                            {t(WEBUI.settings.rotationMinutely)}
                          </SelectItem>
                          <SelectItem value="hourly">
                            {t(WEBUI.settings.rotationHourly)}
                          </SelectItem>
                          <SelectItem value="daily">
                            {t(WEBUI.settings.rotationDaily)}
                          </SelectItem>
                          <SelectItem value="weekly">
                            {t(WEBUI.settings.rotationWeekly)}
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                    {rotationType !== "never" && (
                      <Field>
                        <FieldLabel>{t(WEBUI.settings.maxFiles)}</FieldLabel>
                        <p className="text-xs text-muted-foreground mb-2">
                          {t(WEBUI.settings.maxFilesDesc)}
                        </p>
                        <Input
                          value={maxFiles}
                          onChange={(event) => setMaxFiles(event.target.value)}
                          type="number"
                          min={0}
                          placeholder={t(WEBUI.settings.maxFilesPlaceholder)}
                          className="font-mono"
                        />
                      </Field>
                    )}
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      onClick={handleSaveTopLevelConfig}
                      disabled={isConfigSaving || isRestarting || !isConnected}
                    >
                      {t(WEBUI.common.saveConfig)}
                    </Button>
                    <Button
                      variant="outline"
                      onClick={handleRestartTopLevelConfig}
                      disabled={isConfigSaving || isRestarting || !isConnected}
                    >
                      <RefreshCw className="h-4 w-4 mr-1.5" />
                      {isRestarting
                        ? t(WEBUI.settings.restarting)
                        : t(WEBUI.settings.saveAndRestart)}
                    </Button>
                  </div>
                </CardContent>
              </Card>

              <Card id="upgrade">
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <ArrowUpCircle className="h-5 w-5" />
                    {t(WEBUI.shell.upgrade)}
                  </CardTitle>
                  <CardDescription>
                    {backendSupportsUpgrade === false
                      ? t(WEBUI.settings.upgradeCardDescNoSupport)
                      : t(WEBUI.settings.upgradeCardDescNormal)}
                  </CardDescription>
                </CardHeader>
                <CardContent className="space-y-6">
                  {backendSupportsUpgrade === false && (
                    <div className="flex items-center gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-700 dark:text-amber-400">
                      <CircleAlert className="h-4 w-4 shrink-0" />
                      {t(WEBUI.settings.upgradeNotSupported)}
                    </div>
                  )}

                  {backendSupportsUpgrade !== false && (
                    <div className="grid gap-4 sm:grid-cols-3">
                      <InfoTile
                        label={t(WEBUI.settings.currentVersionLabel)}
                        value={runtimeVersionForCheck || "-"}
                      />
                      <InfoTile
                        label={t(WEBUI.settings.latestVersionLabel)}
                        value={
                          updateInfo
                            ? updateInfo.latestVersion
                            : lastCheckedAt
                              ? "-"
                              : t(WEBUI.settings.notYetChecked)
                        }
                      />
                      <InfoTile
                        label={t(WEBUI.settings.lastCheckedLabel)}
                        value={
                          lastCheckedAt
                            ? new Date(lastCheckedAt).toLocaleTimeString()
                            : "-"
                        }
                      />
                    </div>
                  )}

                  {updateInfo?.updateAvailable && (
                    <div className="flex items-center justify-between gap-3 rounded-lg border border-primary/30 bg-primary/10 px-3 py-2">
                      <div className="flex items-center gap-2 text-sm text-primary">
                        <ArrowUpCircle className="h-4 w-4 shrink-0" />
                        <span>
                          {t(WEBUI.settings.updateFoundMsg, {
                            latest: updateInfo.latestVersion,
                            current: updateInfo.currentVersion,
                          })}
                        </span>
                      </div>
                      <a
                        href={updateInfo.releaseUrl}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="shrink-0 text-xs text-primary underline-offset-2 hover:underline"
                      >
                        {t(WEBUI.settings.releaseNotes)}
                      </a>
                    </div>
                  )}

                  {updateInfo && !updateInfo.updateAvailable && (
                    <div className="flex items-center gap-2 rounded-lg border border-border px-3 py-2 text-sm text-muted-foreground">
                      <CheckCircle2 className="h-4 w-4 shrink-0 text-primary" />
                      {t(WEBUI.settings.alreadyLatest, {
                        version: updateInfo.latestVersion,
                      })}
                    </div>
                  )}

                  {checkError && (
                    <div className="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                      <CircleAlert className="h-4 w-4 shrink-0" />
                      {checkError}
                    </div>
                  )}

                  {applyError && (
                    <div className="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                      <CircleAlert className="h-4 w-4 shrink-0" />
                      {t(WEBUI.settings.upgradeStartFailed, {
                        error: applyError,
                      })}
                    </div>
                  )}

                  {backendSupportsUpgrade !== false && (
                    <div className="flex flex-wrap gap-2">
                      <Button
                        variant="outline"
                        onClick={handleCheckUpdates}
                        disabled={
                          isChecking ||
                          !runtimeVersionForCheck ||
                          backendSupportsUpgrade === null
                        }
                      >
                        <RefreshCw
                          className={`h-4 w-4 mr-1.5 ${isChecking ? "animate-spin" : ""}`}
                        />
                        {isChecking
                          ? t(WEBUI.settings.checkingUpdates)
                          : t(WEBUI.settings.checkUpdates)}
                      </Button>
                      {updateInfo?.updateAvailable && (
                        <Button
                          onClick={() => void triggerUpgrade()}
                          disabled={isApplying || isRestarting}
                        >
                          <ArrowUpCircle className="h-4 w-4 mr-1.5" />
                          {isApplying
                            ? t(WEBUI.settings.upgrading)
                            : t(WEBUI.settings.upgradeNow)}
                        </Button>
                      )}
                    </div>
                  )}

                  <div className="space-y-4">
                    <p className="text-sm font-medium">
                      {t(WEBUI.settings.upgradeConfigSection)}
                    </p>
                    <div className="grid gap-4 sm:grid-cols-2">
                      <Field>
                        <FieldLabel>{t(WEBUI.settings.githubRepo)}</FieldLabel>
                        <p className="text-xs text-muted-foreground mb-2">
                          {t(WEBUI.settings.githubRepoDesc, {
                            default: DEFAULT_UPGRADE_CONFIG.repository,
                          })}
                        </p>
                        <Input
                          value={upgradeConfig.repository}
                          onChange={(e) =>
                            setUpgradeConfig({ repository: e.target.value })
                          }
                          placeholder={DEFAULT_UPGRADE_CONFIG.repository}
                          className="font-mono"
                        />
                      </Field>
                      <Field>
                        <FieldLabel>{t(WEBUI.settings.bundleType)}</FieldLabel>
                        <p className="text-xs text-muted-foreground mb-2">
                          {t(WEBUI.settings.bundleTypeDesc)}
                        </p>
                        <Select
                          value={upgradeConfig.bundle}
                          onValueChange={(v) =>
                            setUpgradeConfig({ bundle: v as UpgradeBundle })
                          }
                        >
                          <SelectTrigger>
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="auto">
                              {t(WEBUI.settings.bundleAuto)}
                            </SelectItem>
                            <SelectItem value="full">
                              {t(WEBUI.settings.bundleFull)}
                            </SelectItem>
                            <SelectItem value="standard">
                              {t(WEBUI.settings.bundleStandard)}
                            </SelectItem>
                            <SelectItem value="minimal">
                              {t(WEBUI.settings.bundleMinimal)}
                            </SelectItem>
                          </SelectContent>
                        </Select>
                      </Field>
                      <Field>
                        <FieldLabel>{t(WEBUI.settings.socks5Proxy)}</FieldLabel>
                        <p className="text-xs text-muted-foreground mb-2">
                          {t(WEBUI.settings.socks5ProxyDesc)}
                        </p>
                        <Input
                          value={upgradeConfig.socks5}
                          onChange={(e) =>
                            setUpgradeConfig({ socks5: e.target.value })
                          }
                          placeholder={t(WEBUI.settings.socks5ProxyPlaceholder)}
                          className="font-mono"
                        />
                      </Field>
                      <Field>
                        <FieldLabel>{t(WEBUI.settings.githubToken)}</FieldLabel>
                        <p className="text-xs text-muted-foreground mb-2">
                          {t(WEBUI.settings.githubTokenDesc)}
                        </p>
                        <Input
                          value={upgradeConfig.githubToken}
                          onChange={(e) =>
                            setUpgradeConfig({ githubToken: e.target.value })
                          }
                          type="password"
                          placeholder={t(WEBUI.settings.githubTokenPlaceholder)}
                          autoComplete="off"
                          autoCapitalize="none"
                          spellCheck={false}
                          className="font-mono"
                        />
                      </Field>
                    </div>
                    <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border px-3 py-2">
                      <div>
                        <p className="text-sm font-medium">
                          {t(WEBUI.settings.persistGithubToken)}
                        </p>
                        <p className="text-xs text-muted-foreground mt-0.5">
                          {t(WEBUI.settings.persistGithubTokenDesc)}
                        </p>
                      </div>
                      <div className="flex items-center gap-2">
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          aria-expanded={tokenPersistenceHelpOpen}
                          onClick={() =>
                            setTokenPersistenceHelpOpen((open) => !open)
                          }
                        >
                          <CircleAlert data-icon="inline-start" />
                          {t(WEBUI.settings.tokenSaveRisk)}
                        </Button>
                        <Switch
                          aria-label={t(WEBUI.settings.persistGithubToken)}
                          checked={upgradeConfig.persistGithubToken}
                          onCheckedChange={(v) =>
                            setUpgradeConfig({ persistGithubToken: v })
                          }
                        />
                      </div>
                    </div>
                    {tokenPersistenceHelpOpen && (
                      <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-700 dark:text-amber-400">
                        <p className="font-medium">
                          {t(WEBUI.settings.tokenPersistenceAdviceTitle)}
                        </p>
                        <p className="mt-1">
                          {t(WEBUI.settings.tokenPersistenceSafe)}
                        </p>
                        <p className="mt-1">
                          {t(WEBUI.settings.tokenPersistenceUnsafe)}
                        </p>
                        <p className="mt-1">
                          {t(WEBUI.settings.tokenPersistenceScope)}
                        </p>
                      </div>
                    )}
                    <div className="flex flex-wrap gap-6">
                      <div className="flex items-center justify-between gap-4">
                        <div>
                          <p className="text-sm font-medium">
                            {t(WEBUI.settings.allowPrerelease)}
                          </p>
                          <p className="text-xs text-muted-foreground mt-0.5">
                            {t(WEBUI.settings.allowPrereleaseDesc)}
                          </p>
                        </div>
                        <Switch
                          checked={upgradeConfig.allowPrerelease}
                          onCheckedChange={(v) =>
                            setUpgradeConfig({ allowPrerelease: v })
                          }
                        />
                      </div>
                      <div className="flex items-center justify-between gap-4">
                        <div>
                          <p className="text-sm font-medium">
                            {t(WEBUI.settings.autoCheck)}
                          </p>
                          <p className="text-xs text-muted-foreground mt-0.5">
                            {t(WEBUI.settings.autoCheckDesc)}
                          </p>
                        </div>
                        <Switch
                          checked={upgradeConfig.autoCheck}
                          onCheckedChange={(v) =>
                            setUpgradeConfig({ autoCheck: v })
                          }
                        />
                      </div>
                    </div>
                  </div>

                  <div className="space-y-2">
                    <p className="text-sm font-medium">
                      {t(WEBUI.settings.cliCommand)}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      {t(WEBUI.settings.cliCommandDesc)}
                    </p>
                    <div className="flex items-center gap-2 rounded-lg border bg-muted/50 px-3 py-2">
                      <code className="flex-1 truncate font-mono text-xs">
                        {buildUpgradeCliCommand()}
                      </code>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        className="shrink-0"
                        onClick={() => void handleCopyCommand()}
                      >
                        {copiedCmd ? (
                          <CheckCircle2 className="h-4 w-4 text-primary" />
                        ) : (
                          <Copy className="h-4 w-4" />
                        )}
                        <span className="sr-only">
                          {t(WEBUI.settings.copyCommand)}
                        </span>
                      </Button>
                    </div>
                  </div>
                </CardContent>
              </Card>
            </>
          )}
        </div>
      </main>
    </>
  );
}

function InfoTile({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 rounded-lg border px-3 py-2">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="mt-1 truncate font-mono text-sm font-semibold">
        {value}
      </div>
    </div>
  );
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}
