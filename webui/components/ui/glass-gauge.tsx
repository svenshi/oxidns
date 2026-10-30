"use client";

import * as React from "react";
import { cn } from "@/lib/utils";

export type GlassGaugeColorScheme =
  | "gradient"
  | "cyan"
  | "blue"
  | "purple"
  | "emerald"
  | "amber"
  | "rose";

export interface GlassGaugeProps extends React.HTMLAttributes<HTMLDivElement> {
  value?: number;
  min?: number;
  max?: number;
  size?: number;
  strokeWidth?: number;
  colorScheme?: GlassGaugeColorScheme;
  showValue?: boolean;
  showGlow?: boolean;
  label?: React.ReactNode;
  sublabel?: React.ReactNode;
  unit?: string;
  formatValue?: (val: number) => string;
  indicatorClassName?: string;
  trackClassName?: string;
  centerClassName?: string;
  children?: React.ReactNode;
}

interface SchemeConfig {
  stops: string[];
  glowColor: string;
  aura: string;
}

const SCHEME_CONFIGS: Record<GlassGaugeColorScheme, SchemeConfig> = {
  gradient: {
    stops: ["#22d3ee", "#60a5fa", "#c084fc"], // from-cyan-400 via-blue-400 to-purple-400 (matches GlassProgress)
    glowColor: "rgba(59, 130, 246, 0.55)",
    aura: "from-cyan-500/25 via-blue-500/25 to-purple-500/25",
  },
  cyan: {
    stops: ["#06b6d4", "#22d3ee"],
    glowColor: "rgba(34, 211, 238, 0.5)",
    aura: "from-cyan-500/25 to-blue-500/25",
  },
  blue: {
    stops: ["#3b82f6", "#60a5fa"],
    glowColor: "rgba(59, 130, 246, 0.5)",
    aura: "from-blue-500/25 to-indigo-500/25",
  },
  purple: {
    stops: ["#8b5cf6", "#c084fc"],
    glowColor: "rgba(168, 85, 247, 0.5)",
    aura: "from-purple-500/25 to-pink-500/25",
  },
  emerald: {
    stops: ["#059669", "#34d399"],
    glowColor: "rgba(16, 185, 129, 0.5)",
    aura: "from-emerald-500/25 to-teal-500/25",
  },
  amber: {
    stops: ["#d97706", "#fbbf24"],
    glowColor: "rgba(245, 158, 11, 0.5)",
    aura: "from-amber-500/25 to-yellow-500/25",
  },
  rose: {
    stops: ["#e11d48", "#fb7185"],
    glowColor: "rgba(244, 63, 94, 0.5)",
    aura: "from-rose-500/25 to-pink-500/25",
  },
};

const GlassGauge = React.forwardRef<HTMLDivElement, GlassGaugeProps>(
  (
    {
      className,
      value = 0,
      min = 0,
      max = 100,
      size = 170,
      strokeWidth = 14,
      colorScheme = "gradient",
      showValue = true,
      showGlow = true,
      label,
      sublabel,
      unit = "%",
      formatValue,
      indicatorClassName,
      trackClassName,
      centerClassName,
      children,
      ...props
    },
    ref,
  ) => {
    const rawId = React.useId();
    const id = rawId.replace(/[^a-zA-Z0-9-_]/g, "");
    const gradientId = `glass-gauge-gradient-${id}`;
    const glowId = `glass-gauge-glow-${id}`;

    const clampedValue = Math.min(max, Math.max(min, Number.isFinite(value) ? value : 0));
    const range = Math.max(0.0001, max - min);
    const progressFraction = Math.min(1, Math.max(0, (clampedValue - min) / range));

    const center = size / 2;
    const radius = Math.max(1, (size - strokeWidth) / 2);
    const circumference = 2 * Math.PI * radius;
    const strokeDashoffset = circumference * (1 - progressFraction);

    const scheme = SCHEME_CONFIGS[colorScheme] || SCHEME_CONFIGS.gradient;

    const displayValue = formatValue
      ? formatValue(clampedValue)
      : `${clampedValue % 1 === 0 ? clampedValue.toFixed(0) : clampedValue.toFixed(1)}${unit}`;

    return (
      <div
        ref={ref}
        className={cn(
          "relative inline-flex items-center justify-center select-none",
          className,
        )}
        style={{ width: size, height: size }}
        {...props}
      >
        {/* Ambient Neon Atmosphere Glow */}
        {showGlow && (
          <div
            className={cn(
              "absolute inset-1 rounded-full blur-xl opacity-40 pointer-events-none bg-gradient-to-tr transition-opacity duration-500",
              scheme.aura,
            )}
          />
        )}

        <svg
          width={size}
          height={size}
          viewBox={`0 0 ${size} ${size}`}
          className="relative block overflow-visible"
        >
          <defs>
            <linearGradient id={gradientId} x1="0%" y1="0%" x2="100%" y2="100%">
              {scheme.stops.length === 3 ? (
                <>
                  <stop offset="0%" stopColor={scheme.stops[0]} />
                  <stop offset="50%" stopColor={scheme.stops[1]} />
                  <stop offset="100%" stopColor={scheme.stops[2]} />
                </>
              ) : (
                <>
                  <stop offset="0%" stopColor={scheme.stops[0]} />
                  <stop offset="100%" stopColor={scheme.stops[1]} />
                </>
              )}
            </linearGradient>

            <filter id={glowId} x="-25%" y="-25%" width="150%" height="150%">
              <feDropShadow
                dx="0"
                dy="0"
                stdDeviation="3.5"
                floodColor={scheme.glowColor}
                floodOpacity="0.65"
              />
            </filter>
          </defs>

          {/* Background Track Circle */}
          <circle
            cx={center}
            cy={center}
            r={radius}
            fill="none"
            stroke="currentColor"
            strokeWidth={strokeWidth}
            className={cn(
              "text-foreground/[0.08] dark:text-white/[0.08] transition-colors",
              trackClassName,
            )}
          />

          {/* Liquid Glass Outer Hairline */}
          <circle
            cx={center}
            cy={center}
            r={radius + strokeWidth / 2}
            fill="none"
            stroke="currentColor"
            strokeWidth={1}
            className="text-foreground/[0.05] dark:text-white/[0.08]"
          />

          {/* Liquid Glass Inner Hairline */}
          <circle
            cx={center}
            cy={center}
            r={Math.max(1, radius - strokeWidth / 2)}
            fill="none"
            stroke="currentColor"
            strokeWidth={1}
            className="text-foreground/[0.05] dark:text-white/[0.08]"
          />

          {/* Animated Active Foreground Arc */}
          <circle
            cx={center}
            cy={center}
            r={radius}
            fill="none"
            stroke={`url(#${gradientId})`}
            strokeWidth={strokeWidth}
            strokeLinecap="round"
            strokeDasharray={circumference}
            strokeDashoffset={strokeDashoffset}
            transform={`rotate(-90 ${center} ${center})`}
            filter={`url(#${glowId})`}
            className={cn(
              "transition-[stroke-dashoffset] duration-700 ease-out",
              indicatorClassName,
            )}
          />
        </svg>

        {/* Center Frosted Liquid Glass Hub */}
        <div
          className={cn(
            "absolute rounded-full flex flex-col items-center justify-center text-center pointer-events-none transition-all",
            "bg-white/40 dark:bg-white/[0.04] backdrop-blur-md border border-white/40 dark:border-white/10",
            "shadow-[inset_0_1px_2px_rgba(255,255,255,0.4),0_4px_16px_rgba(0,0,0,0.04)]",
            centerClassName,
          )}
          style={{
            width: Math.max(0, size - strokeWidth * 2 - 16),
            height: Math.max(0, size - strokeWidth * 2 - 16),
          }}
        >
          {children ? (
            children
          ) : (
            <>
              {showValue && (
                <span className="text-2xl font-bold font-mono tracking-tight text-foreground drop-shadow-sm">
                  {displayValue}
                </span>
              )}
              {label && (
                <span className="text-[11px] font-medium text-muted-foreground mt-0.5">
                  {label}
                </span>
              )}
              {sublabel && (
                <span className="text-[10px] text-muted-foreground/70 mt-0.5">
                  {sublabel}
                </span>
              )}
            </>
          )}
        </div>
      </div>
    );
  },
);

GlassGauge.displayName = "GlassGauge";

export { GlassGauge };
