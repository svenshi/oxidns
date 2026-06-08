"use client";

import { useState } from "react";
import Link from "next/link";
import { PlugZap, FileCode2, Loader2, KeyRound, CircleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Field, FieldLabel } from "@/components/ui/field";
import { useAppStore } from "@/lib/store";
import { useAuthStore } from "@/lib/auth-store";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

export function LoginRequired() {
  const serverConfig = useAuthStore((s) => s.serverConfig);
  const connect = useAuthStore((s) => s.connect);
  const isConnecting = useAuthStore((s) => s.isConnecting);
  const connectionError = useAuthStore((s) => s.connectionError);
  const rememberLogin = useAuthStore((s) => s.rememberLogin);
  const setRememberLogin = useAuthStore((s) => s.setRememberLogin);
  const loadConfig = useAppStore((s) => s.loadConfig);
  const setEditorMode = useAppStore((s) => s.setEditorMode);

  const [username, setUsername] = useState(serverConfig.username);
  const [password, setPassword] = useState("");

  const hadCredentials =
    serverConfig.requiresAuth && serverConfig.username && serverConfig.password;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const ok = await connect({
      ...serverConfig,
      requiresAuth: true,
      username,
      password,
    });
    if (ok) await loadConfig();
  };

  return (
    <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
      <Card className="max-w-sm">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <KeyRound className="h-5 w-5" />
            登录
          </CardTitle>
          <CardDescription>
            {hadCredentials
              ? "登录凭据已失效，请重新输入密码"
              : "此服务已开启身份验证，请输入账号密码继续"}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="rounded-md border bg-muted/40 px-3 py-2 text-xs text-muted-foreground font-mono truncate">
            {serverConfig.url || "/api"}
          </div>
          <form onSubmit={handleSubmit} className="space-y-4">
            <Field>
              <FieldLabel>用户名</FieldLabel>
              <Input
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                autoComplete="username"
                autoFocus
              />
            </Field>
            <Field>
              <FieldLabel>密码</FieldLabel>
              <Input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                autoComplete="current-password"
              />
            </Field>
            <div className="flex items-center justify-between gap-4">
              <label
                htmlFor="remember-login"
                className="cursor-pointer select-none"
              >
                <p className="text-sm font-medium">记住登录状态</p>
                <p className="text-xs text-muted-foreground mt-0.5">
                  关闭后下次访问需重新输入密码
                </p>
              </label>
              <Switch
                id="remember-login"
                checked={rememberLogin}
                onCheckedChange={setRememberLogin}
                aria-label="记住登录状态"
              />
            </div>
            {connectionError && (
              <div className="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                <CircleAlert className="h-4 w-4 shrink-0" />
                {connectionError}
              </div>
            )}
            <Button
              type="submit"
              className="w-full"
              disabled={isConnecting || !username || !password}
            >
              {isConnecting ? "登录中…" : "登录"}
            </Button>
          </form>
          <div className="flex flex-wrap items-center gap-2 border-t pt-4">
            <Button variant="outline" size="sm" asChild>
              <Link href="/settings">修改连接设置</Link>
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setEditorMode(true)}
            >
              <FileCode2 className="h-3.5 w-3.5 mr-1.5" />
              离线编辑配置
            </Button>
          </div>
        </CardContent>
      </Card>
    </main>
  );
}

export function ConnectionPending() {
  return (
    <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
      <Card className="max-w-xl">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Loader2 className="h-5 w-5 animate-spin" />
            正在连接后台服务
          </CardTitle>
          <CardDescription>
            正在通过默认地址连接 OxiDNS 管理 API，请稍候。
          </CardDescription>
        </CardHeader>
      </Card>
    </main>
  );
}

export function ConnectionRequired() {
  const setEditorMode = useAppStore((s) => s.setEditorMode);
  return (
    <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
      <Card className="max-w-xl">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <PlugZap className="h-5 w-5" />
            需要连接后台服务
          </CardTitle>
          <CardDescription>
            当前 WebUI 尚未连接 OxiDNS 管理 API，请先在系统配置中连接后台服务。
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          <Button asChild>
            <Link href="/settings">前往系统配置</Link>
          </Button>
          <Button variant="outline" onClick={() => setEditorMode(true)}>
            <FileCode2 className="h-4 w-4 mr-1.5" />
            离线编辑配置文件
          </Button>
        </CardContent>
      </Card>
    </main>
  );
}
