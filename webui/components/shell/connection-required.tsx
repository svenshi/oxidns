"use client";

import { useState } from "react";
import Link from "next/link";
import {
  PlugZap,
  FileCode2,
  Loader2,
  KeyRound,
  CircleAlert,
} from "lucide-react";
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
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

export function LoginRequired() {
  const { t } = useI18n();
  const serverConfig = useAuthStore((s) => s.serverConfig);
  const connect = useAuthStore((s) => s.connect);
  const isConnecting = useAuthStore((s) => s.isConnecting);
  const connectionError = useAuthStore((s) => s.connectionError);
  const rememberLogin = useAuthStore((s) => s.rememberLogin);
  const setRememberLogin = useAuthStore((s) => s.setRememberLogin);
  const loadConfig = useAppStore((s) => s.loadConfig);
  const setEditorMode = useAppStore((s) => s.setEditorMode);
  const webUiMode = useAppStore((s) => s.webUiMode);
  const editorDisabledInStandardMode = webUiMode === "standard";

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
            {t(WEBUI.connection.loginTitle)}
          </CardTitle>
          <CardDescription>
            {hadCredentials
              ? t(WEBUI.connection.credentialsExpired)
              : t(WEBUI.connection.authRequired)}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="rounded-md border bg-muted/40 px-3 py-2 text-xs text-muted-foreground font-mono truncate">
            {serverConfig.url || "/api"}
          </div>
          <form onSubmit={handleSubmit} className="space-y-4">
            <Field>
              <FieldLabel>{t(WEBUI.connection.username)}</FieldLabel>
              <Input
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                autoComplete="username"
                autoFocus
              />
            </Field>
            <Field>
              <FieldLabel>{t(WEBUI.connection.password)}</FieldLabel>
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
                <p className="text-sm font-medium">
                  {t(WEBUI.connection.rememberLogin)}
                </p>
                <p className="text-xs text-muted-foreground mt-0.5">
                  {t(WEBUI.connection.rememberLoginDesc)}
                </p>
              </label>
              <Switch
                id="remember-login"
                checked={rememberLogin}
                onCheckedChange={setRememberLogin}
                aria-label={t(WEBUI.connection.rememberLogin)}
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
              {isConnecting
                ? t(WEBUI.connection.loggingIn)
                : t(WEBUI.connection.loginTitle)}
            </Button>
          </form>
          <div className="flex flex-wrap items-center gap-2 border-t pt-4">
            <Button variant="outline" size="sm" asChild>
              <Link href="/settings">{t(WEBUI.connection.editConnection)}</Link>
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={editorDisabledInStandardMode}
              title={
                editorDisabledInStandardMode
                  ? t(WEBUI.shell.editorDisabledInStandardMode)
                  : undefined
              }
              onClick={() => setEditorMode(true)}
            >
              <FileCode2 className="h-3.5 w-3.5 mr-1.5" />
              {t(WEBUI.connection.offlineEditConfig)}
            </Button>
          </div>
        </CardContent>
      </Card>
    </main>
  );
}

export function ConnectionPending() {
  const { t } = useI18n();
  return (
    <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
      <Card className="max-w-xl">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Loader2 className="h-5 w-5 animate-spin" />
            {t(WEBUI.connection.pendingTitle)}
          </CardTitle>
          <CardDescription>{t(WEBUI.connection.pendingDesc)}</CardDescription>
        </CardHeader>
      </Card>
    </main>
  );
}

export function ConnectionRequired() {
  const { t } = useI18n();
  const setEditorMode = useAppStore((s) => s.setEditorMode);
  const webUiMode = useAppStore((s) => s.webUiMode);
  const editorDisabledInStandardMode = webUiMode === "standard";
  return (
    <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
      <Card className="max-w-xl">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <PlugZap className="h-5 w-5" />
            {t(WEBUI.connection.requiredTitle)}
          </CardTitle>
          <CardDescription>{t(WEBUI.connection.requiredDesc)}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          <Button asChild>
            <Link href="/settings">{t(WEBUI.connection.goSettings)}</Link>
          </Button>
          <Button
            variant="outline"
            disabled={editorDisabledInStandardMode}
            title={
              editorDisabledInStandardMode
                ? t(WEBUI.shell.editorDisabledInStandardMode)
                : undefined
            }
            onClick={() => setEditorMode(true)}
          >
            <FileCode2 className="h-4 w-4 mr-1.5" />
            {t(WEBUI.connection.offlineEditConfigFile)}
          </Button>
        </CardContent>
      </Card>
    </main>
  );
}
