"use client";

import { useEffect, useState } from "react";
import { useTheme } from "next-themes";
import { useWallpaperStore, WALLPAPER_PRESETS } from "@/lib/wallpaper-store";

export function WallpaperBackground() {
  const { resolvedTheme } = useTheme();
  const currentPresetId = useWallpaperStore((s) => s.currentPresetId);
  const customImageUrl = useWallpaperStore((s) => s.customImageUrl);
  const blurLevel = useWallpaperStore((s) => s.blurLevel);
  const chromaticAberration = useWallpaperStore((s) => s.chromaticAberration);
  const mouseLighting = useWallpaperStore((s) => s.mouseLighting);
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setMounted(true);
  }, []);

  // Sync liquid glass optical physics parameters to document root CSS variables
  useEffect(() => {
    const root = document.documentElement;
    const blurPx = blurLevel === "low" ? "18px" : blurLevel === "high" ? "50px" : "36px";
    root.style.setProperty("--glass-blur", blurPx);

    if (chromaticAberration) {
      root.style.setProperty("--glass-aberration-cyan", "rgba(56, 189, 248, 0.26)");
      root.style.setProperty("--glass-aberration-rose", "rgba(244, 63, 94, 0.22)");
    } else {
      root.style.setProperty("--glass-aberration-cyan", "transparent");
      root.style.setProperty("--glass-aberration-rose", "transparent");
    }
  }, [blurLevel, chromaticAberration]);

  // Global mouse position tracker to drive the liquid glass raytraced specular shimmer
  useEffect(() => {
    if (!mouseLighting) return;
    const handleMouseMove = (e: MouseEvent) => {
      const x = e.clientX;
      const y = e.clientY;
      document.documentElement.style.setProperty("--mouse-x", `${x}px`);
      document.documentElement.style.setProperty("--mouse-y", `${y}px`);
    };

    window.addEventListener("mousemove", handleMouseMove, { passive: true });
    return () => window.removeEventListener("mousemove", handleMouseMove);
  }, [mouseLighting]);

  if (!mounted) {
    // Render an initial unobtrusive background shell during SSR to eliminate visual flashing
    return (
      <div
        className="fixed inset-0 pointer-events-none -z-10 bg-slate-100 dark:bg-[#070a14]"
        aria-hidden="true"
      />
    );
  }

  const isDark = resolvedTheme === "dark";
  const preset =
    WALLPAPER_PRESETS.find((p) => p.id === currentPresetId) || WALLPAPER_PRESETS[0];

  if (preset.id === "none") return null;

  const isPhoto = preset.category === "photography" || (preset.id === "custom" && customImageUrl);
  const photoUrl = preset.id === "custom" ? customImageUrl : preset.imageUrl;
  const bgStyle = isDark ? preset.darkCss : preset.lightCss;

  return (
    <div
      className="fixed inset-0 pointer-events-none -z-10 overflow-hidden transition-all duration-700 ease-out select-none"
      aria-hidden="true"
    >
      {/* ── Mode A: Curated High-Resolution Photography or Custom Image ── */}
      {isPhoto && photoUrl ? (
        <div
          className="absolute inset-0 bg-cover bg-center transition-all duration-1000 ease-in-out"
          style={{ backgroundImage: `url("${photoUrl}")` }}
        />
      ) : (
        /* ── Mode B: Organic Vector Generative Shader Scenes ── */
        <>
          {/* Base gradient canvas */}
          <div
            className="absolute inset-0 transition-all duration-1000 ease-in-out"
            style={{ background: bgStyle }}
          />

          {/* Special Optics Preset: Warm Amber Sun Orb + Cool Cyan Optical Halo */}
          {preset.id === "optics" && (
            <>
              {/* Luminous Warm Golden Sun Core on center-right (mirrors user reference screenshot) */}
              <div
                className="absolute top-[18%] right-[12%] w-[42vw] h-[42vw] max-w-[580px] max-h-[580px] rounded-full blur-[75px] pointer-events-none transition-all duration-1000 animate-pulse"
                style={{
                  background: isDark
                    ? "radial-gradient(circle, rgba(245, 158, 11, 0.48) 0%, rgba(217, 119, 6, 0.22) 50%, transparent 80%)"
                    : "radial-gradient(circle, rgba(251, 191, 36, 0.65) 0%, rgba(245, 158, 11, 0.35) 50%, transparent 80%)",
                  animationDuration: "12s",
                }}
              />

              {/* Luminous Cyan/Emerald Caustic Halo on bottom-left */}
              <div
                className="absolute -bottom-[8%] -left-[5%] w-[48vw] h-[48vw] max-w-[650px] max-h-[650px] rounded-full blur-[85px] pointer-events-none transition-all duration-1000 animate-pulse"
                style={{
                  background: isDark
                    ? "radial-gradient(circle, rgba(6, 182, 212, 0.38) 0%, rgba(16, 185, 129, 0.22) 55%, transparent 75%)"
                    : "radial-gradient(circle, rgba(34, 211, 238, 0.45) 0%, rgba(52, 211, 153, 0.28) 55%, transparent 75%)",
                  animationDuration: "16s",
                }}
              />

              {/* Refractive Optical Lens Ring SVG (Generative vector caustic halo) */}
              <svg
                className="absolute bottom-[2%] left-[2%] w-[55vw] h-[55vw] max-w-[700px] max-h-[700px] pointer-events-none opacity-40 dark:opacity-25"
                viewBox="0 0 500 500"
                fill="none"
              >
                <circle
                  cx="250"
                  cy="250"
                  r="210"
                  stroke={isDark ? "url(#opticsGradDark)" : "url(#opticsGradLight)"}
                  strokeWidth="22"
                  strokeOpacity="0.4"
                  filter="blur(16px)"
                />
                <circle
                  cx="250"
                  cy="250"
                  r="190"
                  stroke={isDark ? "url(#opticsGradDark)" : "url(#opticsGradLight)"}
                  strokeWidth="4"
                  strokeOpacity="0.6"
                />
                <circle
                  cx="250"
                  cy="250"
                  r="140"
                  stroke={isDark ? "#38bdf8" : "#0284c7"}
                  strokeWidth="1.5"
                  strokeOpacity="0.25"
                  strokeDasharray="6 8"
                />
                <defs>
                  <linearGradient id="opticsGradDark" x1="0" y1="0" x2="500" y2="500">
                    <stop offset="0%" stopColor="#38bdf8" />
                    <stop offset="50%" stopColor="#34d399" />
                    <stop offset="100%" stopColor="#fbbf24" />
                  </linearGradient>
                  <linearGradient id="opticsGradLight" x1="0" y1="0" x2="500" y2="500">
                    <stop offset="0%" stopColor="#0284c7" />
                    <stop offset="50%" stopColor="#059669" />
                    <stop offset="100%" stopColor="#d97706" />
                  </linearGradient>
                </defs>
              </svg>

              {/* Rose Aurora Ribbon on upper right */}
              <div
                className="absolute -top-[10%] right-[5%] w-[45vw] h-[220px] rounded-full blur-[70px] pointer-events-none -rotate-12 transition-all duration-1000 opacity-60"
                style={{
                  background: isDark
                    ? "radial-gradient(ellipse, rgba(244, 63, 94, 0.22) 0%, transparent 70%)"
                    : "radial-gradient(ellipse, rgba(251, 113, 133, 0.28) 0%, transparent 70%)",
                }}
              />
            </>
          )}

          {/* Floating Organic Caustic Orbs for other presets */}
          {preset.id !== "optics" && (
            <>
              <div
                className="absolute -top-[15%] -left-[8%] w-[55vw] h-[55vw] rounded-full blur-[100px] opacity-40 animate-pulse pointer-events-none"
                style={{
                  background: isDark
                    ? "radial-gradient(circle, rgba(14, 165, 233, 0.25) 0%, transparent 70%)"
                    : "radial-gradient(circle, rgba(56, 189, 248, 0.2) 0%, transparent 70%)",
                  animationDuration: "14s",
                }}
              />
              <div
                className="absolute -bottom-[15%] -right-[8%] w-[60vw] h-[60vw] rounded-full blur-[120px] opacity-35 animate-pulse pointer-events-none"
                style={{
                  background: isDark
                    ? "radial-gradient(circle, rgba(168, 85, 247, 0.22) 0%, transparent 70%)"
                    : "radial-gradient(circle, rgba(192, 132, 252, 0.18) 0%, transparent 70%)",
                  animationDuration: "18s",
                }}
              />
            </>
          )}
        </>
      )}

      {/* ── Layer 3: Micro-noise Tactile Glass Texture ── */}
      {/* Eliminates color banding on 8-bit displays and gives an authentic tactile Apple glass sheen */}
      <div
        className="absolute inset-0 opacity-[0.025] dark:opacity-[0.038] pointer-events-none mix-blend-overlay"
        style={{
          backgroundImage: `url("data:image/svg+xml,%3Csvg viewBox='0 0 200 200' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noiseFilter'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.8' numOctaves='3' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noiseFilter)'/%3E%3C/svg%3E")`,
        }}
      />

      {/* ── Layer 4: Reading-Optimized Contrast Equalizer ── */}
      {/* Guarantees 100% contrast legibility for all DNS charts and metrics text */}
      <div className="absolute inset-0 bg-white/[0.04] dark:bg-black/[0.18] backdrop-blur-[0.5px] pointer-events-none" />
    </div>
  );
}
