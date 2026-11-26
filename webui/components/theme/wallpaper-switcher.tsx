"use client";

import { useState } from "react";
import {
  Sparkles,
  Check,
  Eye,
  Sliders,
  SunMoon,
  Image as ImageIcon,
  Link as LinkIcon,
} from "lucide-react";
import {
  useWallpaperStore,
  WALLPAPER_PRESETS,
  type WallpaperPresetId,
} from "@/lib/wallpaper-store";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";

export function WallpaperSwitcher() {
  const [open, setOpen] = useState(false);
  const currentPresetId = useWallpaperStore((s) => s.currentPresetId);
  const setPreset = useWallpaperStore((s) => s.setPreset);
  const customImageUrl = useWallpaperStore((s) => s.customImageUrl);
  const setCustomImageUrl = useWallpaperStore((s) => s.setCustomImageUrl);
  const blurLevel = useWallpaperStore((s) => s.blurLevel);
  const setBlurLevel = useWallpaperStore((s) => s.setBlurLevel);
  const chromaticAberration = useWallpaperStore((s) => s.chromaticAberration);
  const setChromaticAberration = useWallpaperStore((s) => s.setChromaticAberration);
  const mouseLighting = useWallpaperStore((s) => s.mouseLighting);
  const setMouseLighting = useWallpaperStore((s) => s.setMouseLighting);

  const [inputUrl, setInputUrl] = useState(customImageUrl);

  const activePreset =
    WALLPAPER_PRESETS.find((p) => p.id === currentPresetId) || WALLPAPER_PRESETS[0];

  const handleApplyCustomUrl = (e: React.FormEvent) => {
    e.preventDefault();
    if (inputUrl.trim()) {
      setCustomImageUrl(inputUrl.trim());
      setPreset("custom");
    }
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="h-8 px-2 sm:px-2.5 rounded-xl border border-white/20 dark:border-white/10 bg-white/40 dark:bg-slate-900/40 hover:bg-white/60 dark:hover:bg-slate-900/60 text-xs font-medium flex items-center gap-1.5 transition-all text-foreground shadow-xs backdrop-blur-md"
          title="切换阅读背景与液态玻璃光学特效"
        >
          <Sparkles className="h-3.5 w-3.5 text-primary animate-pulse" />
          <span className="hidden sm:inline font-medium">玻璃特效</span>
          <span className="text-[10px] text-muted-foreground font-normal hidden md:inline">
            ({activePreset.name})
          </span>
        </button>
      </PopoverTrigger>

      <PopoverContent
        align="end"
        sideOffset={8}
        className="w-[calc(100vw-1.5rem)] sm:w-[340px] max-w-[340px] p-3.5 sm:p-4 liquid-glass-card border border-white/30 dark:border-white/10 shadow-2xl space-y-4 max-h-[85vh] overflow-y-auto"
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border/30 pb-2.5">
          <div className="flex items-center gap-2">
            <div className="h-6 w-6 rounded-lg bg-primary/15 text-primary flex items-center justify-center">
              <Sparkles className="h-3.5 w-3.5" />
            </div>
            <div>
              <h4 className="text-xs font-bold text-foreground">液态玻璃与护眼背景</h4>
              <p className="text-[10px] text-muted-foreground">适合长时间专注阅读的清润背景</p>
            </div>
          </div>
          <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-primary/10 text-primary border border-primary/20 font-mono font-medium">
            Liquid Glass
          </span>
        </div>

        {/* Category: 艺术光感画卷 */}
        <div className="space-y-1.5">
          <label className="text-[11px] font-semibold text-muted-foreground flex items-center gap-1.5">
            <SunMoon className="h-3 w-3 text-primary" />
            <span>艺术折射流光</span>
          </label>
          <div className="grid grid-cols-2 gap-1.5">
            {WALLPAPER_PRESETS.filter((p) => p.category === "artistic").map((preset) => {
              const isSelected = preset.id === currentPresetId;
              return (
                <button
                  key={preset.id}
                  type="button"
                  onClick={() => setPreset(preset.id as WallpaperPresetId)}
                  className={cn(
                    "flex items-center gap-2 p-1.5 rounded-xl border text-left transition-all text-xs group relative overflow-hidden",
                    isSelected
                      ? "bg-primary/15 border-primary/40 text-foreground font-semibold shadow-xs"
                      : "bg-white/30 dark:bg-slate-900/30 hover:bg-white/50 dark:hover:bg-slate-900/50 border-border/40 text-muted-foreground hover:text-foreground",
                  )}
                >
                  <div
                    className="h-5 w-5 rounded-lg shrink-0 border border-white/20 shadow-xs flex items-center justify-center text-[10px] text-white font-bold"
                    style={{ background: preset.previewGradient }}
                  >
                    {isSelected && <Check className="h-3 w-3 text-white drop-shadow-md" />}
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[11px]">{preset.name}</div>
                  </div>
                </button>
              );
            })}
          </div>
        </div>

        {/* Category: 高清实景画卷 */}
        <div className="space-y-1.5 pt-1 border-t border-border/30">
          <label className="text-[11px] font-semibold text-muted-foreground flex items-center gap-1.5">
            <ImageIcon className="h-3 w-3 text-emerald-500" />
            <span>高清护眼实景</span>
          </label>
          <div className="grid grid-cols-2 gap-1.5">
            {WALLPAPER_PRESETS.filter((p) => p.category === "photography").map((preset) => {
              const isSelected = preset.id === currentPresetId;
              return (
                <button
                  key={preset.id}
                  type="button"
                  onClick={() => setPreset(preset.id as WallpaperPresetId)}
                  className={cn(
                    "flex items-center gap-2 p-1.5 rounded-xl border text-left transition-all text-xs group relative overflow-hidden",
                    isSelected
                      ? "bg-primary/15 border-primary/40 text-foreground font-semibold shadow-xs"
                      : "bg-white/30 dark:bg-slate-900/30 hover:bg-white/50 dark:hover:bg-slate-900/50 border-border/40 text-muted-foreground hover:text-foreground",
                  )}
                >
                  <div
                    className="h-5 w-5 rounded-lg shrink-0 border border-white/20 shadow-xs flex items-center justify-center text-[10px] text-white font-bold"
                    style={{ background: preset.previewGradient }}
                  >
                    {isSelected && <Check className="h-3 w-3 text-white drop-shadow-md" />}
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[11px]">{preset.name}</div>
                  </div>
                </button>
              );
            })}
          </div>
        </div>

        {/* Category: 极简与自定义 */}
        <div className="space-y-2 pt-1 border-t border-border/30">
          <label className="text-[11px] font-semibold text-muted-foreground flex items-center gap-1.5">
            <LinkIcon className="h-3 w-3 text-purple-400" />
            <span>极简与自定义壁纸</span>
          </label>
          <div className="grid grid-cols-2 gap-1.5">
            {WALLPAPER_PRESETS.filter((p) => p.category === "minimal").map((preset) => {
              const isSelected = preset.id === currentPresetId;
              return (
                <button
                  key={preset.id}
                  type="button"
                  onClick={() => setPreset(preset.id as WallpaperPresetId)}
                  className={cn(
                    "flex items-center gap-2 p-1.5 rounded-xl border text-left transition-all text-xs group relative overflow-hidden",
                    isSelected
                      ? "bg-primary/15 border-primary/40 text-foreground font-semibold shadow-xs"
                      : "bg-white/30 dark:bg-slate-900/30 hover:bg-white/50 dark:hover:bg-slate-900/50 border-border/40 text-muted-foreground hover:text-foreground",
                  )}
                >
                  <div
                    className="h-5 w-5 rounded-lg shrink-0 border border-white/20 shadow-xs flex items-center justify-center text-[10px] text-white font-bold"
                    style={{ background: preset.previewGradient }}
                  >
                    {isSelected && <Check className="h-3 w-3 text-white drop-shadow-md" />}
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[11px]">{preset.name}</div>
                  </div>
                </button>
              );
            })}
          </div>

          {/* Custom image URL input box */}
          {currentPresetId === "custom" && (
            <form onSubmit={handleApplyCustomUrl} className="flex gap-1.5 pt-1">
              <input
                type="url"
                placeholder="粘贴图片直链 (https://...)"
                value={inputUrl}
                onChange={(e) => setInputUrl(e.target.value)}
                className="flex-1 text-xs px-2.5 py-1 rounded-lg border border-border/60 bg-white/40 dark:bg-slate-900/40 text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary font-mono text-[11px]"
              />
              <button
                type="submit"
                className="px-2.5 py-1 rounded-lg bg-primary text-primary-foreground text-xs font-medium hover:bg-primary/90 transition-colors shrink-0 shadow-xs"
              >
                应用
              </button>
            </form>
          )}
        </div>

        {/* Liquid Glass Optical Effects Controls */}
        <div className="space-y-2 pt-1 border-t border-border/30">
          <label className="text-[11px] font-semibold text-muted-foreground flex items-center gap-1.5">
            <Sliders className="h-3 w-3 text-primary" />
            <span>物理玻璃光学特性</span>
          </label>

          {/* Blur Depth */}
          <div className="flex items-center justify-between text-xs">
            <span className="text-muted-foreground text-[11px]">折射模糊深度</span>
            <div className="flex items-center gap-1 bg-white/20 dark:bg-slate-900/40 p-0.5 rounded-lg border border-border/40 font-mono text-[10px]">
              {(["low", "medium", "high"] as const).map((lvl) => (
                <button
                  key={lvl}
                  type="button"
                  onClick={() => setBlurLevel(lvl)}
                  className={cn(
                    "px-2 py-0.5 rounded-md transition-all",
                    blurLevel === lvl
                      ? "bg-primary text-primary-foreground font-medium shadow-xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {lvl === "low" ? "轻柔" : lvl === "medium" ? "标准" : "深邃"}
                </button>
              ))}
            </div>
          </div>

          {/* Chromatic Aberration Toggle */}
          <div className="flex items-center justify-between text-xs pt-0.5">
            <span className="text-muted-foreground text-[11px]">边缘色散折射 (三棱镜分色)</span>
            <button
              type="button"
              onClick={() => setChromaticAberration(!chromaticAberration)}
              className={cn(
                "h-5 w-9 rounded-full transition-colors relative flex items-center p-0.5",
                chromaticAberration ? "bg-primary" : "bg-muted",
              )}
            >
              <div
                className={cn(
                  "h-4 w-4 rounded-full bg-white transition-transform shadow-xs",
                  chromaticAberration ? "translate-x-4" : "translate-x-0",
                )}
              />
            </button>
          </div>

          {/* Mouse Light Shimmer Toggle */}
          <div className="flex items-center justify-between text-xs pt-0.5">
            <span className="text-muted-foreground text-[11px]">光线跟随 (鼠标高光漫反射)</span>
            <button
              type="button"
              onClick={() => setMouseLighting(!mouseLighting)}
              className={cn(
                "h-5 w-9 rounded-full transition-colors relative flex items-center p-0.5",
                mouseLighting ? "bg-primary" : "bg-muted",
              )}
            >
              <div
                className={cn(
                  "h-4 w-4 rounded-full bg-white transition-transform shadow-xs",
                  mouseLighting ? "translate-x-4" : "translate-x-0",
                )}
              />
            </button>
          </div>
        </div>

        {/* Footer info */}
        <div className="pt-2 border-t border-border/30 text-[10px] text-muted-foreground flex items-center justify-between">
          <span className="flex items-center gap-1">
            <Eye className="h-3 w-3 text-emerald-500" />
            双层遮罩高对比度防眩光
          </span>
          <span className="font-mono text-[10px] opacity-75">ybouane/liquidglass</span>
        </div>
      </PopoverContent>
    </Popover>
  );
}
