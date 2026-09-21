"use client";

import * as React from "react";
import { AlertTriangle, CircleX, X } from "lucide-react";
import { Toast as ToastPrimitive } from "radix-ui";

import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";

export type ToastVariant = "warning" | "error";

export interface ToastInput {
  title: string;
  description?: string;
  variant: ToastVariant;
  duration?: number;
}

interface ToastItem extends ToastInput {
  id: number;
}

interface ToastContextValue {
  toast: (input: ToastInput) => number;
  dismiss: (id: number) => void;
}

const ToastContext = React.createContext<ToastContextValue | null>(null);

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const { t } = useI18n();
  const nextId = React.useRef(1);
  const [items, setItems] = React.useState<ToastItem[]>([]);

  const dismiss = React.useCallback((id: number) => {
    setItems((current) => current.filter((item) => item.id !== id));
  }, []);

  const toast = React.useCallback((input: ToastInput) => {
    const id = nextId.current++;
    setItems((current) => [...current, { ...input, id }]);
    return id;
  }, []);

  const value = React.useMemo(() => ({ toast, dismiss }), [toast, dismiss]);

  return (
    <ToastContext.Provider value={value}>
      <ToastPrimitive.Provider duration={5000} swipeDirection="right">
        {children}
        {items.map((item) => {
          const Icon = item.variant === "warning" ? AlertTriangle : CircleX;
          return (
            <ToastPrimitive.Root
              key={item.id}
              open
              duration={item.duration}
              onOpenChange={(open) => {
                if (!open) dismiss(item.id);
              }}
              className={cn(
                "grid w-full grid-cols-[auto_1fr_auto] items-start gap-x-2 rounded-lg border bg-popover p-3 text-popover-foreground shadow-lg outline-none",
                "data-[state=open]:animate-in data-[state=open]:slide-in-from-right-full data-[state=closed]:animate-out data-[state=closed]:fade-out-80 data-[swipe=move]:translate-x-(--radix-toast-swipe-move-x) data-[swipe=cancel]:translate-x-0 data-[swipe=end]:animate-out data-[swipe=end]:translate-x-(--radix-toast-swipe-end-x)",
                item.variant === "warning"
                  ? "border-warning/50"
                  : "border-destructive/50",
              )}
            >
              <Icon
                className={cn(
                  "mt-0.5 size-4",
                  item.variant === "warning"
                    ? "text-warning"
                    : "text-destructive",
                )}
              />
              <div className="min-w-0">
                <ToastPrimitive.Title className="text-sm font-medium">
                  {item.title}
                </ToastPrimitive.Title>
                {item.description && (
                  <ToastPrimitive.Description className="mt-1 text-xs text-muted-foreground">
                    {item.description}
                  </ToastPrimitive.Description>
                )}
              </div>
              <ToastPrimitive.Close
                className="rounded-sm p-1 text-muted-foreground outline-none transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                aria-label={t(WEBUI.common.close)}
              >
                <X className="size-3.5" />
              </ToastPrimitive.Close>
            </ToastPrimitive.Root>
          );
        })}
        <ToastPrimitive.Viewport className="fixed right-4 top-4 z-[100] flex max-h-svh w-[min(24rem,calc(100vw-2rem))] flex-col gap-2 outline-none" />
      </ToastPrimitive.Provider>
    </ToastContext.Provider>
  );
}

export function useToast() {
  const context = React.useContext(ToastContext);
  if (!context) {
    throw new Error("useToast must be used within ToastProvider");
  }
  return context;
}
