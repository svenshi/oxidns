"use client";

import { useState } from "react";
import Image from "next/image";
import {
  Eye,
  EyeOff,
  Loader2,
  CircleAlert,
  Settings2,
  FileCode2,
  RotateCcw,
  Sun,
  Moon,
  Monitor,
  Languages,
} from "lucide-react";
import { useTheme } from "next-themes";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { useAppStore } from "@/lib/store";
import { useAuthStore } from "@/lib/auth-store";
import { WallpaperSwitcher } from "@/components/theme/wallpaper-switcher";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";

export function LoginView() {
  const { t, locale, toggleLocale } = useI18n();
  const { theme, setTheme, resolvedTheme } = useTheme();

  const serverConfig = useAuthStore((s) => s.serverConfig);
  const connect = useAuthStore((s) => s.connect);
  const isConnecting = useAuthStore((s) => s.isConnecting);
  const connectionError = useAuthStore((s) => s.connectionError);
  const rememberLogin = useAuthStore((s) => s.rememberLogin);
  const setRememberLogin = useAuthStore((s) => s.setRememberLogin);

  const loadConfig = useAppStore((s) => s.loadConfig);
  const setEditorMode = useAppStore((s) => s.setEditorMode);

  const [username, setUsername] = useState(serverConfig.username || "");
  const [password, setPassword] = useState(serverConfig.password || "");
  const [serverUrl, setServerUrl] = useState(serverConfig.url || "/api");
  const [showPassword, setShowPassword] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);

  const hadCredentials =
    serverConfig.requiresAuth && serverConfig.username && !serverConfig.password;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!username.trim() || !password || isConnecting) return;

    const ok = await connect({
      ...serverConfig,
      url: serverUrl.trim() || "/api",
      requiresAuth: true,
      username: username.trim(),
      password,
    });

    if (ok) {
      await loadConfig();
    }
  };

  const cycleTheme = () => {
    if (theme === "system") {
      setTheme("light");
    } else if (theme === "light") {
      setTheme("dark");
    } else {
      setTheme("system");
    }
  };

  return (
    <div className="min-h-svh w-full flex flex-col items-center justify-center p-4 relative overflow-hidden select-none">
      {/* Background radial glow */}
      <div className="absolute -top-40 -left-40 size-96 rounded-full bg-emerald-500/10 dark:bg-emerald-400/5 blur-3xl pointer-events-none" />
      <div className="absolute -bottom-40 -right-40 size-96 rounded-full bg-teal-500/10 dark:bg-teal-400/5 blur-3xl pointer-events-none" />

      {/* Top right floating toolbar */}
      <div className="absolute top-4 right-4 z-20 flex items-center gap-1.5 p-1 rounded-2xl liquid-glass-panel border border-white/30 dark:border-white/10 shadow-lg backdrop-blur-xl">
        <WallpaperSwitcher />

        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="size-8 rounded-xl text-muted-foreground hover:text-foreground"
              onClick={cycleTheme}
              aria-label="Toggle theme"
            >
              {theme === "system" ? (
                <Monitor className="size-4" />
              ) : resolvedTheme === "dark" ? (
                <Moon className="size-4" />
              ) : (
                <Sun className="size-4" />
              )}
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom">
            <span>
              {theme === "system"
                ? "跟随系统"
                : resolvedTheme === "dark"
                  ? "暗色主题"
                  : "亮色主题"}
            </span>
          </TooltipContent>
        </Tooltip>

        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="size-8 rounded-xl text-muted-foreground hover:text-foreground"
              onClick={toggleLocale}
              aria-label="Toggle language"
            >
              <Languages className="size-4" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom">
            <span>{locale === "zh-CN" ? "切换至 English" : "Switch to 中文"}</span>
          </TooltipContent>
        </Tooltip>
      </div>

      {/* Centered Login Card */}
      <div className="w-full max-w-[420px] z-10 animate-in fade-in zoom-in-95 duration-300">
        <div className="liquid-glass-card rounded-2xl md:rounded-3xl p-7 md:p-9 shadow-2xl border border-white/40 dark:border-white/10 backdrop-blur-2xl bg-card/70 dark:bg-card/45 relative">
          
          {/* Card Header matching screenshot layout */}
          <div className="flex items-center gap-4 mb-7">
            {/* App Logo */}
            <div className="relative size-14 shrink-0 flex items-center justify-center rounded-2xl bg-gradient-to-br from-emerald-500/15 via-teal-500/10 to-primary/15 border border-emerald-500/30 dark:border-emerald-400/25 shadow-md shadow-emerald-500/10 overflow-hidden">
              <Image
                src="/logo-light.png"
                alt="OxiDNS"
                width={36}
                height={36}
                className="size-9 object-contain dark:hidden transition-transform duration-300 hover:scale-105"
                priority
              />
              <Image
                src="/logo-dark.png"
                alt="OxiDNS"
                width={36}
                height={36}
                className="hidden size-9 object-contain dark:block transition-transform duration-300 hover:scale-105"
                priority
              />
            </div>

            {/* Title & Subtitle */}
            <div className="flex flex-col min-w-0">
              <h1 className="text-2xl md:text-3xl font-bold tracking-tight text-foreground leading-tight">
                {t(WEBUI.connection.loginTitle)}
              </h1>
              <p className="text-xs md:text-sm text-muted-foreground mt-1 truncate">
                {t(WEBUI.connection.loginSubtitle) || "登录 OxiDNS 仪表盘"}
              </p>
            </div>
          </div>

          {/* Form */}
          <form onSubmit={handleSubmit} className="space-y-4">
            {/* Username Input */}
            <div className="space-y-1.5">
              <div className="relative">
                <Input
                  type="text"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  placeholder={t(WEBUI.connection.username)}
                  autoComplete="username"
                  autoFocus={!username}
                  className="h-12 px-4 rounded-xl border-white/40 dark:border-white/15 bg-background/50 dark:bg-background/40 backdrop-blur-md text-foreground placeholder:text-muted-foreground/60 focus-visible:ring-emerald-500/30 focus-visible:border-emerald-500 transition-all text-sm md:text-base"
                />
              </div>
            </div>

            {/* Password Input with Eye Toggle */}
            <div className="space-y-1.5">
              <div className="relative flex items-center">
                <Input
                  type={showPassword ? "text" : "password"}
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={t(WEBUI.connection.password)}
                  autoComplete="current-password"
                  autoFocus={Boolean(username)}
                  className="h-12 pl-4 pr-11 rounded-xl border-white/40 dark:border-white/15 bg-background/50 dark:bg-background/40 backdrop-blur-md text-foreground placeholder:text-muted-foreground/60 focus-visible:ring-emerald-500/30 focus-visible:border-emerald-500 transition-all text-sm md:text-base"
                />
                <button
                  type="button"
                  onClick={() => setShowPassword((prev) => !prev)}
                  aria-label={t(WEBUI.connection.togglePasswordVisibility)}
                  title={t(WEBUI.connection.togglePasswordVisibility)}
                  className="absolute right-3 p-1.5 rounded-lg text-muted-foreground/70 hover:text-foreground hover:bg-muted/50 transition-colors focus:outline-none cursor-pointer"
                >
                  {showPassword ? (
                    <Eye className="size-5" />
                  ) : (
                    <EyeOff className="size-5" />
                  )}
                </button>
              </div>
            </div>

            {/* Controls: Remember Login & Advanced Settings */}
            <div className="flex items-center justify-between pt-1">
              <label className="flex items-center gap-2 cursor-pointer select-none text-xs md:text-sm text-muted-foreground hover:text-foreground transition-colors">
                <Checkbox
                  checked={rememberLogin}
                  onCheckedChange={(checked) => setRememberLogin(checked === true)}
                  id="remember-login"
                  className="rounded-md border-white/40 dark:border-white/20 data-[state=checked]:bg-emerald-500 data-[state=checked]:border-emerald-500"
                />
                <span>{t(WEBUI.connection.rememberLogin)}</span>
              </label>

              <button
                type="button"
                onClick={() => setShowAdvanced((v) => !v)}
                className="text-xs text-muted-foreground/80 hover:text-primary transition-colors inline-flex items-center gap-1 cursor-pointer"
              >
                <Settings2 className="size-3.5" />
                <span>
                  {showAdvanced
                    ? t(WEBUI.connection.hideAdvancedSettings)
                    : t(WEBUI.connection.advancedSettings)}
                </span>
              </button>
            </div>

            {/* Collapsible Advanced Settings (Server URL) */}
            {showAdvanced && (
              <div className="p-3.5 rounded-xl border border-white/30 dark:border-white/10 bg-background/40 backdrop-blur-sm space-y-2.5 text-xs animate-in fade-in slide-in-from-top-2 duration-200">
                <div className="flex items-center justify-between text-muted-foreground">
                  <span>{t(WEBUI.connection.serverAddress)}:</span>
                  <button
                    type="button"
                    onClick={() => setServerUrl("/api")}
                    title="重置为默认 /api"
                    className="hover:text-foreground inline-flex items-center gap-1 text-[11px]"
                  >
                    <RotateCcw className="size-3" />
                    <span>默认</span>
                  </button>
                </div>
                <Input
                  type="text"
                  value={serverUrl}
                  onChange={(e) => setServerUrl(e.target.value)}
                  placeholder="/api 或 http://localhost:8080"
                  className="h-9 text-xs font-mono rounded-lg bg-background/60"
                />
              </div>
            )}

            {/* Error Message Alert */}
            {(connectionError || (hadCredentials && !connectionError)) && (
              <div className="flex items-center gap-2.5 rounded-xl border border-destructive/30 bg-destructive/10 px-3.5 py-2.5 text-xs md:text-sm text-destructive animate-in fade-in slide-in-from-top-1 duration-200">
                <CircleAlert className="size-4 shrink-0" />
                <span className="leading-snug">
                  {connectionError || t(WEBUI.connection.credentialsExpired)}
                </span>
              </div>
            )}

            {/* Submit Button */}
            <Button
              type="submit"
              disabled={isConnecting || !username.trim() || !password}
              className="w-full h-12 rounded-xl text-base font-medium tracking-wide bg-emerald-500 hover:bg-emerald-600 active:bg-emerald-700 text-white shadow-lg shadow-emerald-500/25 transition-all duration-200 disabled:opacity-50 disabled:shadow-none hover:shadow-emerald-500/35 cursor-pointer mt-2"
            >
              {isConnecting ? (
                <span className="flex items-center justify-center gap-2">
                  <Loader2 className="size-4 animate-spin" />
                  {t(WEBUI.connection.loggingIn)}
                </span>
              ) : (
                t(WEBUI.connection.loginTitle)
              )}
            </Button>
          </form>

          {/* Footer Actions */}
          <div className="flex items-center justify-center gap-4 mt-6 pt-5 border-t border-white/20 dark:border-white/10 text-xs text-muted-foreground">
            <button
              type="button"
              onClick={() => setEditorMode(true)}
              className="hover:text-foreground inline-flex items-center gap-1.5 transition-colors cursor-pointer"
            >
              <FileCode2 className="size-3.5" />
              <span>{t(WEBUI.connection.offlineEditConfig)}</span>
            </button>
          </div>
        </div>

        {/* Brand note below card */}
        <p className="text-center text-[11px] text-muted-foreground/60 mt-4 tracking-wider">
          OxiDNS · Next-Gen DNS Engine & Management Console
        </p>
      </div>
    </div>
  );
}
