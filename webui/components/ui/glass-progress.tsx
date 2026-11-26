"use client";

import * as React from "react";
import { Progress as ProgressPrimitive } from "radix-ui";

import { cn } from "@/lib/utils";

interface GlassProgressProps
  extends React.ComponentPropsWithoutRef<typeof ProgressPrimitive.Root> {
  value?: number;
  indicatorClassName?: string;
  showGlow?: boolean;
}

const GlassProgress = React.forwardRef<
  React.ElementRef<typeof ProgressPrimitive.Root>,
  GlassProgressProps
>(({ className, indicatorClassName, value = 0, showGlow = true, ...props }, ref) => {
  const safeValue = Math.min(100, Math.max(0, value || 0));

  return (
    <div className="relative w-full">
      {showGlow && (
        <div className="absolute -inset-0.5 rounded-full bg-gradient-to-r from-cyan-500/25 via-blue-500/25 to-purple-500/25 blur-[3px] opacity-50 pointer-events-none" />
      )}
      <ProgressPrimitive.Root
        ref={ref}
        className={cn(
          "relative h-2 w-full overflow-hidden rounded-full bg-white/10 dark:bg-white/[0.06] backdrop-blur-xl border border-white/20 dark:border-white/10",
          className,
        )}
        {...props}
      >
        <div
          className={cn(
            "h-full transition-all duration-500 ease-out rounded-full bg-gradient-to-r from-cyan-400 via-blue-400 to-purple-400 shadow-[0_0_12px_rgba(59,130,246,0.5)]",
            indicatorClassName,
          )}
          style={{ width: `${safeValue}%` }}
        />
      </ProgressPrimitive.Root>
    </div>
  );
});

GlassProgress.displayName = "GlassProgress";

export { GlassProgress };
