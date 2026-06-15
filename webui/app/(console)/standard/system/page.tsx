"use client";

import { useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import { AppHeader } from "@/components/shell/app-header";
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
import { Switch } from "@/components/ui/switch";
import { isSameServerIdentity, useAuthStore } from "@/lib/auth-store";
import { stringifyOxiDnsConfig, type OxiDnsConfig } from "@/lib/oxidns-config";
import { useAppStore } from "@/lib/store";
import { useUpdateStore } from "@/lib/update-store";
import {
  ArrowUpCircle,
  CheckCircle2,
  CircleAlert,
  FileCode2,
  PlugZap,
  RefreshCw,
  ScrollText,
  Server,
  ShieldCheck,
  SlidersHorizontal,
} from "lucide-react";

type AuthInfo = {
  enabled: boolean;
  username: string;
};

type AuthEditMode = null | "enable" | "change" | "disable";

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function getAuthInfo(config: OxiDnsConfig): AuthInfo {
  const api = asRecord(config.api);
  const http = api.http;
  const httpObj = typeof http === "string" ? {} : asRecord(http);
  const auth = asRecord(httpObj.auth);
  return {
    enabled: auth.type === "basic",
    username: String(auth.username ?? ""),
  };
}

function buildConfigWithAuth(
  config: OxiDnsConfig,
  enabled: boolean,
  username: string,
  password: string,
): OxiDnsConfig {
  const nextApi = { ...asRecord(config.api) };
  const currentHttp = nextApi.http;

  if (typeof currentHttp === "string") {
    nextApi.http = enabled
      ? {
          listen: currentHttp,
          auth: {
            type: "basic",
            username: username.trim(),
            password,
          },
        }
      : currentHttp;
  } else {
    const nextHttp = { ...asRecord(currentHttp) };
    if (!nextHttp.listen) nextHttp.listen = ":9199";
    if (enabled) {
      nextHttp.auth = {
        type: "basic",
        username: username.trim(),
        password,
      };
    } else {
      delete nextHttp.auth;
    }
    nextApi.http = nextHttp;
  }

  return {
    ...config,
    api: Object.keys(nextApi).length > 0 ? nextApi : undefined,
  };
}

function buildConfigWithLogLevel(
  config: OxiDnsConfig,
  level: string,
): OxiDnsConfig {
  return {
    ...config,
    log: {
      ...asRecord(config.log),
      level,
    },
  };
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

export default function StandardSystemPage() {
  const router = useRouter();
  const serverConfig = useAuthStore((s) => s.serverConfig);
  const setServerConfig = useAuthStore((s) => s.setServerConfig);
  const connect = useAuthStore((s) => s.connect);
  const isConnected = useAuthStore((s) => s.isConnected);
  const isConnecting = useAuthStore((s) => s.isConnecting);
  const connectionError = useAuthStore((s) => s.connectionError);

  const configModel = useAppStore((s) => s.configModel);
  const configPath = useAppStore((s) => s.configPath);
  const configVersion = useAppStore((s) => s.configVersion);
  const configError = useAppStore((s) => s.configError);
  const health = useAppStore((s) => s.health);
  const system = useAppStore((s) => s.system);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const reloadStatus = useAppStore((s) => s.reloadStatus);
  const webUiMode = useAppStore((s) => s.webUiMode);
  const setWebUiMode = useAppStore((s) => s.setWebUiMode);
  const setYamlConfig = useAppStore((s) => s.setYamlConfig);
  const loadConfig = useAppStore((s) => s.loadConfig);
  const resetBackendSession = useAppStore((s) => s.resetBackendSession);
  const restartApp = useAppStore((s) => s.restartApp);
  const isRestarting = useAppStore((s) => s.isRestarting);

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

  const [draftBackendUrl, setDraftBackendUrl] = useState<string | null>(null);
  const [authEditMode, setAuthEditMode] = useState<AuthEditMode>(null);
  const [newAuthUsername, setNewAuthUsername] = useState("");
  const [newAuthPassword, setNewAuthPassword] = useState("");
  const [confirmAuthPassword, setConfirmAuthPassword] = useState("");
  const [draftLogLevel, setDraftLogLevel] = useState<string | null>(null);

  const authInfo = useMemo(() => getAuthInfo(configModel), [configModel]);
  const backendUrl = draftBackendUrl ?? serverConfig.url;
  const logLevel =
    draftLogLevel ?? String(asRecord(configModel.log).level ?? "info");
  const canConnect = backendUrl.trim().length > 0;
  const backendSupportsUpgrade =
    buildInfo != null
      ? buildInfo.enabled_features.includes("plugin-upgrade")
      : null;
  const runtimeVersion = system?.build
    ? `${system.build.version} (${system.build.bundle})`
    : health?.build_bundle
      ? `${health.version} (${health.build_bundle})`
      : (system?.version ?? health?.version ?? "-");
  const runtimeVersionForCheck = system?.build
    ? system.build.version
    : (system?.version ?? health?.version ?? "");

  const applyServerConfig = (nextConfig: typeof serverConfig) => {
    const backendChanged = !isSameServerIdentity(serverConfig, nextConfig);
    setServerConfig(nextConfig);
    if (backendChanged) resetBackendSession();
  };

  const handleSaveConnection = () => {
    applyServerConfig({ ...serverConfig, url: backendUrl.trim() });
    setDraftBackendUrl(null);
  };

  const handleConnect = async () => {
    const nextConfig = { ...serverConfig, url: backendUrl.trim() };
    applyServerConfig(nextConfig);
    setDraftBackendUrl(null);
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

  const resetAuthForm = () => {
    setAuthEditMode(null);
    setNewAuthUsername("");
    setNewAuthPassword("");
    setConfirmAuthPassword("");
  };

  const handleAuthSave = async (
    enabled: boolean,
    username: string,
    password: string,
  ) => {
    const nextConfig = buildConfigWithAuth(
      configModel,
      enabled,
      username,
      password,
    );
    setYamlConfig(stringifyOxiDnsConfig(nextConfig));
    applyServerConfig(
      enabled && username.trim()
        ? {
            ...serverConfig,
            requiresAuth: true,
            username: username.trim(),
            password,
          }
        : {
            ...serverConfig,
            requiresAuth: false,
            username: "",
            password: "",
          },
    );
    resetAuthForm();
    await restartApp();
  };

  const handleSaveLogLevel = async () => {
    setYamlConfig(
      stringifyOxiDnsConfig(buildConfigWithLogLevel(configModel, logLevel)),
    );
    setDraftLogLevel(null);
    await restartApp();
  };

  const handleCheckUpdates = () => {
    if (runtimeVersionForCheck) {
      void checkForUpdates(runtimeVersionForCheck);
    }
  };

  return (
    <>
      <AppHeader
        title="系统"
        breadcrumbs={[{ label: "标准模式", href: "/standard" }]}
      />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-4xl space-y-6">
          <div>
            <h1 className="text-xl font-semibold tracking-tight">系统设置</h1>
            <p className="mt-1 text-sm text-muted-foreground">
              标准模式只保留连接、账号、运行状态、日志级别和应用升级这些常用系统操作。
            </p>
          </div>

          <Card>
            <CardHeader>
              <div className="flex items-start justify-between gap-3">
                <div>
                  <CardTitle className="flex items-center gap-2">
                    <PlugZap className="h-5 w-5" />
                    后台连接
                  </CardTitle>
                  <CardDescription className="mt-1.5">
                    修改 WebUI 连接的 OxiDNS 管理 API 地址。
                  </CardDescription>
                </div>
                <Badge
                  variant="outline"
                  className={
                    isConnected
                      ? "border-primary/30 bg-primary/10 text-primary"
                      : "bg-muted text-muted-foreground"
                  }
                >
                  {isConnected ? "已连接" : "未连接"}
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-4">
              <Field>
                <FieldLabel>服务地址</FieldLabel>
                <Input
                  value={backendUrl}
                  onChange={(event) => setDraftBackendUrl(event.target.value)}
                  placeholder="/api 或 http://127.0.0.1:9199/api"
                  className="font-mono"
                />
              </Field>
              {connectionError ? (
                <div className="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                  <CircleAlert className="h-4 w-4 shrink-0" />
                  {connectionError}
                </div>
              ) : null}
              <div className="flex flex-wrap gap-2">
                <Button onClick={handleSaveConnection}>保存地址</Button>
                <Button
                  variant="outline"
                  onClick={handleConnect}
                  disabled={!canConnect || isConnecting}
                >
                  <PlugZap className="h-4 w-4" />
                  {isConnecting ? "连接中" : "重新连接"}
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
                    标准模式使用表单读写 YAML；专家模式提供插件中心、拓扑和完整
                    YAML 控制台。
                  </CardDescription>
                </div>
                <Badge variant="secondary">
                  {webUiMode === "standard" ? "标准模式" : "专家模式"}
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="grid gap-3 sm:grid-cols-2">
              <div className="rounded-lg border p-4">
                <div className="font-medium">标准模式</div>
                <p className="mt-1 text-sm leading-6 text-muted-foreground">
                  用开关和输入框完成日常 DNS 配置，隐藏专家级实现细节。
                </p>
                <Button
                  className="mt-4"
                  variant={webUiMode === "standard" ? "secondary" : "default"}
                  disabled={webUiMode === "standard"}
                  onClick={enterStandardMode}
                >
                  {webUiMode === "standard" ? "当前模式" : "切换到标准模式"}
                </Button>
              </div>
              <div className="rounded-lg border p-4">
                <div className="flex items-center gap-2 font-medium">
                  <FileCode2 className="h-4 w-4" />
                  专家模式
                </div>
                <p className="mt-1 text-sm leading-6 text-muted-foreground">
                  进入完整控制台管理插件、历史版本和 YAML 配置。
                </p>
                <Button
                  className="mt-4"
                  variant={webUiMode === "expert" ? "secondary" : "outline"}
                  disabled={webUiMode === "expert"}
                  onClick={enterExpertMode}
                >
                  {webUiMode === "expert" ? "当前模式" : "切换到专家模式"}
                </Button>
              </div>
            </CardContent>
          </Card>

          {isConnected ? (
            <>
              <Card>
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <ShieldCheck className="h-5 w-5" />
                    账号与安全
                  </CardTitle>
                  <CardDescription>
                    管理 WebUI 访问账号。修改后会保存配置并重启 OxiDNS。
                  </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                  {authEditMode === null && (
                    <>
                      {authInfo.enabled ? (
                        <div className="flex flex-wrap items-center justify-between gap-3">
                          <div className="flex flex-wrap items-center gap-2">
                            <Badge
                              variant="outline"
                              className="border-primary/30 bg-primary/10 text-primary"
                            >
                              已启用认证
                            </Badge>
                            <span className="text-sm text-muted-foreground">
                              当前账号{" "}
                              <span className="font-mono font-medium text-foreground">
                                {authInfo.username || "-"}
                              </span>
                            </span>
                          </div>
                          <div className="flex flex-wrap gap-2">
                            <Button
                              variant="outline"
                              size="sm"
                              disabled={isRestarting}
                              onClick={() => {
                                setNewAuthUsername(authInfo.username);
                                setNewAuthPassword("");
                                setConfirmAuthPassword("");
                                setAuthEditMode("change");
                              }}
                            >
                              修改账号密码
                            </Button>
                            <Button
                              variant="outline"
                              size="sm"
                              disabled={isRestarting}
                              onClick={() => setAuthEditMode("disable")}
                            >
                              关闭认证
                            </Button>
                          </div>
                        </div>
                      ) : (
                        <div className="space-y-3">
                          <div className="flex items-center gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-700 dark:text-amber-400">
                            <CircleAlert className="h-4 w-4 shrink-0" />
                            当前未启用访问认证，建议至少设置一个管理账号。
                          </div>
                          <Button
                            size="sm"
                            disabled={isRestarting}
                            onClick={() => {
                              setNewAuthUsername("");
                              setNewAuthPassword("");
                              setConfirmAuthPassword("");
                              setAuthEditMode("enable");
                            }}
                          >
                            设置账号密码
                          </Button>
                        </div>
                      )}
                    </>
                  )}

                  {(authEditMode === "enable" || authEditMode === "change") && (
                    <form
                      className="space-y-4"
                      onSubmit={(event) => {
                        event.preventDefault();
                        void handleAuthSave(
                          true,
                          newAuthUsername,
                          newAuthPassword,
                        );
                      }}
                    >
                      <Field>
                        <FieldLabel>用户名</FieldLabel>
                        <Input
                          value={newAuthUsername}
                          onChange={(event) =>
                            setNewAuthUsername(event.target.value)
                          }
                          autoComplete="username"
                          className="max-w-xs"
                        />
                      </Field>
                      <div className="grid gap-4 sm:grid-cols-2">
                        <Field>
                          <FieldLabel>
                            {authEditMode === "change" ? "新密码" : "密码"}
                          </FieldLabel>
                          <Input
                            type="password"
                            value={newAuthPassword}
                            onChange={(event) =>
                              setNewAuthPassword(event.target.value)
                            }
                            autoComplete="new-password"
                          />
                        </Field>
                        <Field>
                          <FieldLabel>确认密码</FieldLabel>
                          <Input
                            type="password"
                            value={confirmAuthPassword}
                            onChange={(event) =>
                              setConfirmAuthPassword(event.target.value)
                            }
                            autoComplete="new-password"
                          />
                        </Field>
                      </div>
                      {confirmAuthPassword.length > 0 &&
                      newAuthPassword !== confirmAuthPassword ? (
                        <p className="text-sm text-destructive">
                          两次输入的密码不一致。
                        </p>
                      ) : null}
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
                          <RefreshCw className="h-4 w-4" />
                          {isRestarting ? "重启中" : "保存并重启"}
                        </Button>
                        <Button
                          type="button"
                          variant="outline"
                          disabled={isRestarting}
                          onClick={resetAuthForm}
                        >
                          取消
                        </Button>
                      </div>
                    </form>
                  )}

                  {authEditMode === "disable" && (
                    <div className="space-y-4">
                      <div className="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                        <CircleAlert className="h-4 w-4 shrink-0" />
                        关闭认证后，能访问管理 API 的人都可以修改配置。
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <Button
                          variant="destructive"
                          disabled={isRestarting}
                          onClick={() => void handleAuthSave(false, "", "")}
                        >
                          <RefreshCw className="h-4 w-4" />
                          {isRestarting ? "重启中" : "确认关闭并重启"}
                        </Button>
                        <Button
                          variant="outline"
                          disabled={isRestarting}
                          onClick={resetAuthForm}
                        >
                          取消
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
                    运行状态
                  </CardTitle>
                  <CardDescription>
                    只展示定位问题最常用的运行信息。
                  </CardDescription>
                </CardHeader>
                <CardContent className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                  <InfoTile label="版本" value={runtimeVersion} />
                  <InfoTile
                    label="平台"
                    value={system ? `${system.os}/${system.arch}` : "-"}
                  />
                  <InfoTile label="健康状态" value={health?.status ?? "-"} />
                  <InfoTile
                    label="重载状态"
                    value={reloadStatus?.status ?? "-"}
                  />
                  <InfoTile label="配置文件" value={configPath} />
                  <InfoTile
                    label="配置版本"
                    value={configVersion?.slice(0, 12) ?? "-"}
                  />
                  <div className="sm:col-span-2">
                    <Badge
                      variant={configError ? "destructive" : "outline"}
                      className={
                        configError
                          ? ""
                          : "border-primary/30 bg-primary/10 text-primary"
                      }
                    >
                      {configError ? (
                        <CircleAlert className="h-3 w-3" />
                      ) : (
                        <CheckCircle2 className="h-3 w-3" />
                      )}
                      {configError ?? "配置状态正常"}
                    </Badge>
                  </div>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <ScrollText className="h-5 w-5" />
                    日志级别
                  </CardTitle>
                  <CardDescription>
                    标准模式只调整日志级别；文件路径和轮转策略请在专家模式中配置。
                  </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                  <Field className="max-w-xs">
                    <FieldLabel>日志级别</FieldLabel>
                    <Select value={logLevel} onValueChange={setDraftLogLevel}>
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {["trace", "debug", "info", "warn", "error", "off"].map(
                          (level) => (
                            <SelectItem key={level} value={level}>
                              {level}
                            </SelectItem>
                          ),
                        )}
                      </SelectContent>
                    </Select>
                  </Field>
                  <Button onClick={handleSaveLogLevel} disabled={isRestarting}>
                    <RefreshCw className="h-4 w-4" />
                    {isRestarting ? "重启中" : "保存并重启"}
                  </Button>
                </CardContent>
              </Card>

              <Card id="upgrade">
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <ArrowUpCircle className="h-5 w-5" />
                    应用升级
                  </CardTitle>
                  <CardDescription>
                    检查 OxiDNS 新版本并触发后端升级。仓库、代理和 token
                    等高级参数请在专家模式中配置。
                  </CardDescription>
                </CardHeader>
                <CardContent className="space-y-5">
                  {backendSupportsUpgrade === false ? (
                    <div className="flex items-center gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-700 dark:text-amber-400">
                      <CircleAlert className="h-4 w-4 shrink-0" />
                      当前构建不支持在线升级。
                    </div>
                  ) : (
                    <>
                      <div className="grid gap-4 sm:grid-cols-3">
                        <InfoTile
                          label="当前版本"
                          value={runtimeVersionForCheck || "-"}
                        />
                        <InfoTile
                          label="最新版本"
                          value={
                            updateInfo
                              ? updateInfo.latestVersion
                              : lastCheckedAt
                                ? "-"
                                : "尚未检查"
                          }
                        />
                        <InfoTile
                          label="上次检查"
                          value={
                            lastCheckedAt
                              ? new Date(lastCheckedAt).toLocaleString()
                              : "-"
                          }
                        />
                      </div>

                      {updateInfo?.updateAvailable ? (
                        <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-primary/30 bg-primary/10 px-3 py-2 text-sm text-primary">
                          <span>
                            发现新版本 {updateInfo.latestVersion}，当前版本{" "}
                            {updateInfo.currentVersion}。
                          </span>
                          <a
                            href={updateInfo.releaseUrl}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="text-xs underline-offset-2 hover:underline"
                          >
                            查看发布说明
                          </a>
                        </div>
                      ) : null}

                      {updateInfo && !updateInfo.updateAvailable ? (
                        <div className="flex items-center gap-2 rounded-lg border px-3 py-2 text-sm text-muted-foreground">
                          <CheckCircle2 className="h-4 w-4 shrink-0 text-primary" />
                          当前已是最新版本 {updateInfo.latestVersion}。
                        </div>
                      ) : null}

                      {checkError ? (
                        <div className="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                          <CircleAlert className="h-4 w-4 shrink-0" />
                          {checkError}
                        </div>
                      ) : null}

                      {applyError ? (
                        <div className="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                          <CircleAlert className="h-4 w-4 shrink-0" />
                          升级启动失败：{applyError}
                        </div>
                      ) : null}

                      <div className="flex flex-wrap items-center gap-3">
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
                            className={`h-4 w-4 ${isChecking ? "animate-spin" : ""}`}
                          />
                          {isChecking ? "检查中" : "检查更新"}
                        </Button>
                        {updateInfo?.updateAvailable ? (
                          <Button
                            onClick={() => void triggerUpgrade()}
                            disabled={isApplying || isRestarting}
                          >
                            <ArrowUpCircle className="h-4 w-4" />
                            {isApplying ? "升级中" : "立即升级"}
                          </Button>
                        ) : null}
                        <label className="ml-auto flex items-center gap-2 text-sm">
                          <Switch
                            checked={upgradeConfig.autoCheck}
                            onCheckedChange={(checked) =>
                              setUpgradeConfig({ autoCheck: checked })
                            }
                          />
                          自动检查
                        </label>
                      </div>
                    </>
                  )}
                </CardContent>
              </Card>
            </>
          ) : null}
        </div>
      </main>
    </>
  );
}
