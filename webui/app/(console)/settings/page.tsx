"use client";

import { useEffect, useState } from "react";
import { AppHeader } from "@/components/shell/app-header";
import { useAppStore } from "@/lib/store";
import { useAuthStore } from "@/lib/auth-store";
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
  CheckCircle2,
  CircleAlert,
  Cpu,
  FileCode2,
  Globe,
  LogOut,
  PlugZap,
  RefreshCw,
  ScrollText,
  Server,
  ShieldCheck,
} from "lucide-react";

export default function SettingsPage() {
  const serverConfig = useAuthStore((s) => s.serverConfig);
  const setServerConfig = useAuthStore((s) => s.setServerConfig);
  const connect = useAuthStore((s) => s.connect);
  const logout = useAuthStore((s) => s.logout);
  const isConnected = useAuthStore((s) => s.isConnected);
  const isConnecting = useAuthStore((s) => s.isConnecting);
  const connectionError = useAuthStore((s) => s.connectionError);

  const configModel = useAppStore((s) => s.configModel);
  const configPath = useAppStore((s) => s.configPath);
  const configVersion = useAppStore((s) => s.configVersion);
  const configError = useAppStore((s) => s.configError);
  const dependencyGraph = useAppStore((s) => s.dependencyGraph);
  const health = useAppStore((s) => s.health);
  const system = useAppStore((s) => s.system);
  const reloadStatus = useAppStore((s) => s.reloadStatus);
  const setYamlConfig = useAppStore((s) => s.setYamlConfig);
  const saveConfig = useAppStore((s) => s.saveConfig);
  const loadConfig = useAppStore((s) => s.loadConfig);
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const isRestarting = useAppStore((s) => s.isRestarting);
  const restartApp = useAppStore((s) => s.restartApp);

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
  // "账号与安全" card local form state
  const [authEditMode, setAuthEditMode] = useState<null | "enable" | "change" | "disable">(null);
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

  const handleSaveConnection = () => {
    setServerConfig({ ...serverConfig, url: backendUrl.trim() });
  };

  const handleConnect = async () => {
    const nextConfig = { ...serverConfig, url: backendUrl.trim() };
    setServerConfig(nextConfig);
    const ok = await connect(nextConfig);
    if (ok) await loadConfig();
  };

  type AuthOverride = { enabled: boolean; username: string; password: string };

  const buildApiHttpConfig = (authOverride?: AuthOverride): unknown => {
    const authEnabled = authOverride !== undefined ? authOverride.enabled : apiAuthEnabled;
    const authUsername = authOverride !== undefined ? authOverride.username : apiAuthUsername;
    const authPassword = authOverride !== undefined ? authOverride.password : apiAuthPassword;

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
        ? { type: "basic", username: authUsername.trim(), password: authPassword }
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
  const handleAuthSave = async (enabled: boolean, uname: string, pwd: string) => {
    const override: AuthOverride = { enabled, username: uname, password: pwd };
    setYamlConfig(stringifyOxiDnsConfig(buildTopLevelConfig(override)));

    if (enabled && uname.trim()) {
      setServerConfig({ ...serverConfig, requiresAuth: true, username: uname.trim(), password: pwd });
    } else {
      setServerConfig({ ...serverConfig, requiresAuth: false, username: "", password: "" });
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
      <AppHeader title="系统配置" />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="max-w-4xl space-y-6">
          <Card>
            <CardHeader>
              <div className="flex items-start justify-between gap-3">
                <div>
                  <CardTitle className="flex items-center gap-2">
                    <PlugZap className="h-5 w-5" />
                    后台服务
                  </CardTitle>
                  <CardDescription className="mt-1.5">
                    配置 WebUI 连接的后端地址，认证凭据通过登录流程自动管理
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
                  {isConnected ? "已连接" : "未连接"}
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-4">
              <Field>
                <FieldLabel>服务地址</FieldLabel>
                <Input
                  value={backendUrl}
                  onChange={(event) => setBackendUrl(event.target.value)}
                  placeholder="/api 或 http://localhost:8080"
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
                <Button onClick={handleSaveConnection}>保存地址</Button>
                <Button
                  variant="outline"
                  onClick={handleConnect}
                  disabled={!canConnect || isConnecting}
                >
                  <PlugZap className="h-4 w-4 mr-1.5" />
                  {isConnecting ? "连接中" : "重新连接"}
                </Button>
              </div>
            </CardContent>
          </Card>

          {isConnected && (<>

          {/* ── 账号与安全 ─────────────────────────────────── */}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <ShieldCheck className="h-5 w-5" />
                账号与安全
              </CardTitle>
              <CardDescription>
                管理 API 的身份验证，修改账号密码后需重启生效
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
                          已启用
                        </Badge>
                        <span className="text-sm text-muted-foreground">
                          账号：
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
                          修改密码
                        </Button>
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={() => setAuthEditMode("disable")}
                          disabled={isRestarting}
                        >
                          关闭认证
                        </Button>
                      </div>
                    </div>
                  ) : (
                    <div className="space-y-3">
                      <div className="flex items-center gap-2 rounded-lg border border-yellow-500/30 bg-yellow-500/10 px-3 py-2 text-sm text-yellow-700 dark:text-yellow-400">
                        <CircleAlert className="h-4 w-4 shrink-0" />
                        未开启身份验证，任何可访问后台地址的用户均可操作
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
                        设置账号密码
                      </Button>
                    </div>
                  )}
                </>
              )}

              {(authEditMode === "enable" || authEditMode === "change") && (
                <form
                  onSubmit={(e) => {
                    e.preventDefault();
                    void handleAuthSave(true, newAuthUsername, newAuthPassword);
                  }}
                  className="space-y-4"
                >
                  <Field>
                    <FieldLabel>用户名</FieldLabel>
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
                        {authEditMode === "change" ? "新密码" : "密码"}
                      </FieldLabel>
                      <Input
                        type="password"
                        value={newAuthPassword}
                        onChange={(e) => setNewAuthPassword(e.target.value)}
                        autoComplete="new-password"
                      />
                    </Field>
                    <Field>
                      <FieldLabel>确认密码</FieldLabel>
                      <Input
                        type="password"
                        value={confirmAuthPassword}
                        onChange={(e) => setConfirmAuthPassword(e.target.value)}
                        autoComplete="new-password"
                      />
                    </Field>
                  </div>
                  {confirmAuthPassword.length > 0 &&
                    newAuthPassword !== confirmAuthPassword && (
                      <p className="text-sm text-destructive">两次密码不一致</p>
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
                      {isRestarting ? "重启中…" : "保存并重启"}
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      onClick={() => setAuthEditMode(null)}
                      disabled={isRestarting}
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
                    关闭后所有人可无限制访问管理 API，请确认
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      variant="destructive"
                      onClick={() => void handleAuthSave(false, "", "")}
                      disabled={isRestarting}
                    >
                      <RefreshCw className="h-4 w-4 mr-1.5" />
                      {isRestarting ? "重启中…" : "确认关闭并重启"}
                    </Button>
                    <Button
                      variant="outline"
                      onClick={() => setAuthEditMode(null)}
                      disabled={isRestarting}
                    >
                      取消
                    </Button>
                  </div>
                </div>
              )}
            </CardContent>
          </Card>

          {/* ── 运行状态 ─────────────────────────────────── */}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Server className="h-5 w-5" />
                运行状态
              </CardTitle>
              <CardDescription>
                来自 /system、/health 和 /reload/status
              </CardDescription>
            </CardHeader>
            <CardContent className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
              <InfoTile label="编译版本" value={runtimeVersion} />
              <InfoTile
                label="平台"
                value={system ? `${system.os}/${system.arch}` : "-"}
              />
              <InfoTile label="健康状态" value={health?.status ?? "-"} />
              <InfoTile label="重载状态" value={reloadStatus?.status ?? "-"} />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <FileCode2 className="h-5 w-5" />
                配置摘要
              </CardTitle>
            </CardHeader>
            <CardContent className="grid gap-4 sm:grid-cols-2">
              <InfoTile label="配置文件" value={configPath} />
              <InfoTile
                label="版本"
                value={configVersion?.slice(0, 12) ?? "-"}
              />
              <InfoTile
                label="插件数"
                value={String(
                  dependencyGraph?.nodes.length ?? configModel.plugins.length,
                )}
              />
              <InfoTile
                label="初始化顺序"
                value={String(dependencyGraph?.init_order.length ?? 0)}
              />
              <div className="sm:col-span-2">
                <Badge
                  variant={configError ? "destructive" : "outline"}
                  className={configError ? "" : "bg-primary/10 text-primary"}
                >
                  {configError ? (
                    <CircleAlert className="h-3 w-3 mr-1" />
                  ) : (
                    <CheckCircle2 className="h-3 w-3 mr-1" />
                  )}
                  {configError ?? "配置校验通过"}
                </Badge>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Cpu className="h-5 w-5" />
                运行时
              </CardTitle>
              <CardDescription>Tokio 运行时参数（runtime）</CardDescription>
            </CardHeader>
            <CardContent className="space-y-5">
              <Field>
                <FieldLabel>Worker 线程数</FieldLabel>
                <p className="text-xs text-muted-foreground mb-2">
                  Tokio 多线程运行时的 worker
                  数，留空自动取系统可用并行度，不能为 0
                </p>
                <Input
                  value={workerThreads}
                  onChange={(event) => setWorkerThreads(event.target.value)}
                  type="number"
                  min={1}
                  placeholder="留空使用系统默认"
                  className="font-mono max-w-xs"
                />
              </Field>
              <div className="flex flex-wrap gap-2">
                <Button
                  onClick={handleSaveTopLevelConfig}
                  disabled={isConfigSaving || isRestarting || !isConnected}
                >
                  保存配置
                </Button>
                <Button
                  variant="outline"
                  onClick={handleRestartTopLevelConfig}
                  disabled={isConfigSaving || isRestarting || !isConnected}
                >
                  <RefreshCw className="h-4 w-4 mr-1.5" />
                  {isRestarting ? "重启中…" : "保存并重启"}
                </Button>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Globe className="h-5 w-5" />
                管理 API
              </CardTitle>
              <CardDescription>HTTP 管理接口配置（api.http）</CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              {/* 监听地址 */}
              <div className="space-y-2">
                <p className="text-sm font-medium">监听地址 (listen)</p>
                <p className="text-xs text-muted-foreground">
                  支持 <span className="font-mono">ip:port</span>、
                  <span className="font-mono">[ipv6]:port</span>、
                  <span className="font-mono">:port</span>；
                  <span className="font-mono">:port</span> 绑定为双栈{" "}
                  <span className="font-mono">[::]:port</span>，仅监听 IPv4 时写{" "}
                  <span className="font-mono">0.0.0.0:port</span>
                </p>
                <Input
                  value={apiListen}
                  onChange={(e) => setApiListen(e.target.value)}
                  placeholder=":9199"
                  className="font-mono"
                />
              </div>

              {/* TLS */}
              <div className="space-y-4">
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <p className="text-sm font-medium">TLS / SSL</p>
                    <p className="text-xs text-muted-foreground mt-1">
                      配置 HTTPS，cert 与 key 必须成对出现
                    </p>
                  </div>
                  <Switch
                    checked={apiSslEnabled}
                    onCheckedChange={setApiSslEnabled}
                    aria-label="启用 TLS"
                  />
                </div>
                {apiSslEnabled && (
                  <div className="space-y-4">
                    <div className="grid gap-4 sm:grid-cols-2">
                      <Field>
                        <FieldLabel>证书文件路径 (cert)</FieldLabel>
                        <Input
                          value={apiSslCert}
                          onChange={(e) => setApiSslCert(e.target.value)}
                          placeholder="/etc/oxidns/api.crt"
                          className="font-mono"
                        />
                      </Field>
                      <Field>
                        <FieldLabel>私钥文件路径 (key)</FieldLabel>
                        <Input
                          value={apiSslKey}
                          onChange={(e) => setApiSslKey(e.target.value)}
                          placeholder="/etc/oxidns/api.key"
                          className="font-mono"
                        />
                      </Field>
                      <Field>
                        <FieldLabel>客户端 CA 证书 (client_ca)</FieldLabel>
                        <p className="text-xs text-muted-foreground mb-2">
                          可选，启用双向 TLS 时提供
                        </p>
                        <Input
                          value={apiSslClientCa}
                          onChange={(e) => setApiSslClientCa(e.target.value)}
                          placeholder="/etc/oxidns/client-ca.crt"
                          className="font-mono"
                        />
                      </Field>
                    </div>
                    <div className="flex items-start justify-between gap-4">
                      <div>
                        <p className="text-sm font-medium">
                          要求客户端证书 (require_client_cert)
                        </p>
                        <p className="text-xs text-muted-foreground mt-1">
                          启用时必须提供 client_ca，客户端须携带受信任证书
                        </p>
                      </div>
                      <Switch
                        checked={apiSslRequireClientCert}
                        onCheckedChange={setApiSslRequireClientCert}
                        aria-label="要求客户端证书"
                      />
                    </div>
                  </div>
                )}
              </div>

              {/* 身份认证 — 只读，在「账号与安全」中管理 */}
              <div className="flex items-center justify-between gap-4">
                <div>
                  <p className="text-sm font-medium">身份认证 (auth)</p>
                  <p className="text-xs text-muted-foreground mt-1">
                    在上方「账号与安全」中设置账号密码
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
                  {apiAuthEnabled ? `已启用 — ${apiAuthUsername}` : "未启用"}
                </Badge>
              </div>

              {/* CORS */}
              <div className="space-y-2">
                <p className="text-sm font-medium">
                  跨域白名单 (cors.allowed_origins)
                </p>
                <p className="text-xs text-muted-foreground">
                  每行一个 Origin，如{" "}
                  <span className="font-mono">http://localhost:3000</span>； 写{" "}
                  <span className="font-mono">*</span> 允许任意
                  Origin（不可与浏览器凭据跨域同用）；
                  留空时根据监听地址自动推导
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

              {/* WebUI 静态文件 */}
              <div className="space-y-4">
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <p className="text-sm font-medium">
                      WebUI 静态文件 (webui)
                    </p>
                    <p className="text-xs text-muted-foreground mt-1">
                      启用后 WebUI 挂载在 <span className="font-mono">/</span>
                      ，管理 API 位于 <span className="font-mono">/api/*</span>
                    </p>
                  </div>
                  <Switch
                    checked={apiWebuiEnabled}
                    onCheckedChange={setApiWebuiEnabled}
                    aria-label="挂载 WebUI 静态文件"
                  />
                </div>
                {apiWebuiEnabled && (
                  <div className="grid gap-4 sm:grid-cols-2">
                    <Field>
                      <FieldLabel>静态文件目录 (root)</FieldLabel>
                      <Input
                        value={apiWebuiRoot}
                        onChange={(e) => setApiWebuiRoot(e.target.value)}
                        placeholder="/etc/oxidns/webui"
                        className="font-mono"
                      />
                    </Field>
                    <Field>
                      <FieldLabel>首页文件名 (index)</FieldLabel>
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
                  保存配置
                </Button>
                <Button
                  variant="outline"
                  onClick={handleRestartTopLevelConfig}
                  disabled={isConfigSaving || isRestarting || !isConnected}
                >
                  <RefreshCw className="h-4 w-4 mr-1.5" />
                  {isRestarting ? "重启中…" : "保存并重启"}
                </Button>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <ScrollText className="h-5 w-5" />
                日志
              </CardTitle>
              <CardDescription>日志输出与轮转配置（log）</CardDescription>
            </CardHeader>
            <CardContent className="space-y-5">
              <div className="grid gap-4 sm:grid-cols-2">
                <Field>
                  <FieldLabel>日志级别</FieldLabel>
                  <p className="text-xs text-muted-foreground mb-2">
                    控制输出的最低日志级别，默认{" "}
                    <span className="font-mono">info</span>
                  </p>
                  <Select value={logLevel} onValueChange={setLogLevel}>
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
                <Field>
                  <FieldLabel>日志文件路径</FieldLabel>
                  <p className="text-xs text-muted-foreground mb-2">
                    可选，留空仅输出到标准输出；配置后同时写入文件（纯文本，无颜色码）
                  </p>
                  <Input
                    value={logFile}
                    onChange={(event) => setLogFile(event.target.value)}
                    placeholder="留空输出到控制台"
                    className="font-mono"
                  />
                </Field>
                <Field>
                  <FieldLabel>日志轮转策略</FieldLabel>
                  <p className="text-xs text-muted-foreground mb-2">
                    按时间周期自动创建新日志文件，默认不轮转
                  </p>
                  <Select value={rotationType} onValueChange={setRotationType}>
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="never">never — 不轮转</SelectItem>
                      <SelectItem value="minutely">
                        minutely — 按分钟
                      </SelectItem>
                      <SelectItem value="hourly">hourly — 按小时</SelectItem>
                      <SelectItem value="daily">daily — 按天</SelectItem>
                      <SelectItem value="weekly">weekly — 按周</SelectItem>
                    </SelectContent>
                  </Select>
                </Field>
                {rotationType !== "never" && (
                  <Field>
                    <FieldLabel>最多保留历史文件数</FieldLabel>
                    <p className="text-xs text-muted-foreground mb-2">
                      <span className="font-mono">0</span>{" "}
                      或留空表示不自动删除旧文件
                    </p>
                    <Input
                      value={maxFiles}
                      onChange={(event) => setMaxFiles(event.target.value)}
                      type="number"
                      min={0}
                      placeholder="0（不限制）"
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
                  保存配置
                </Button>
                <Button
                  variant="outline"
                  onClick={handleRestartTopLevelConfig}
                  disabled={isConfigSaving || isRestarting || !isConnected}
                >
                  <RefreshCw className="h-4 w-4 mr-1.5" />
                  {isRestarting ? "重启中…" : "保存并重启"}
                </Button>
              </div>
            </CardContent>
          </Card>

          </>)}
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
