import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface PluginCardItem {
  key: string;
  label: string;
  value: string;
}

export function pluginCardGridColumnClass(itemCount: number): string {
  return itemCount < 4 ? "grid-cols-1" : "grid-cols-2";
}

export function pluginCardItemColumnClass(emphasizeValues: boolean): string {
  return emphasizeValues
    ? "grid-cols-[minmax(0,1fr)_auto]"
    : "grid-cols-[minmax(5.25rem,52%)_minmax(0,1fr)]";
}

export function PluginCardItemSurface({ children }: { children: ReactNode }) {
  return (
    <div className="flex h-[5.25rem] items-center overflow-hidden rounded-md bg-muted/25 px-3 py-2">
      {children}
    </div>
  );
}

export function PluginCardItemGrid({
  items,
  emphasizeValues = false,
}: {
  items: PluginCardItem[];
  emphasizeValues?: boolean;
}) {
  return (
    <div
      className={cn(
        "grid w-full gap-x-5 gap-y-1",
        pluginCardGridColumnClass(items.length),
      )}
    >
      {items.map((item) => (
        <div
          key={item.key}
          className={cn(
            "grid h-5 min-w-0 items-center gap-2 text-xs leading-5",
            pluginCardItemColumnClass(emphasizeValues),
          )}
        >
          <span
            className="min-w-0 truncate text-muted-foreground"
            title={item.label}
          >
            {item.label}
          </span>
          <span
            className={cn(
              "max-w-[9.5rem] min-w-0 truncate text-right font-mono text-xs tracking-[-0.01em] text-foreground tabular-nums",
              emphasizeValues ? "font-semibold" : "font-normal",
            )}
            title={item.value}
          >
            {item.value}
          </span>
        </div>
      ))}
    </div>
  );
}
