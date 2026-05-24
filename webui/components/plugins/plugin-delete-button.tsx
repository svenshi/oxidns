"use client";

import { useState, type ComponentProps } from "react";
import { AlertTriangle, ArrowRightLeft, Pencil, Trash2 } from "lucide-react";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useAppStore, type PluginDeletePreview } from "@/lib/store";
import type { PluginInstance } from "@/lib/types";
import { cn } from "@/lib/utils";

interface PluginDeleteButtonProps {
  plugin: PluginInstance;
  className?: string;
  iconClassName?: string;
  label?: string;
  variant?: ComponentProps<typeof Button>["variant"];
  size?: ComponentProps<typeof Button>["size"];
  stopPropagation?: boolean;
  onDeleted?: () => void;
}

export function PluginDeleteButton({
  plugin,
  className,
  iconClassName,
  label,
  variant = "ghost",
  size = "icon",
  stopPropagation = true,
  onDeleted,
}: PluginDeleteButtonProps) {
  const previewPluginDelete = useAppStore((s) => s.previewPluginDelete);
  const confirmDeletePlugin = useAppStore((s) => s.confirmDeletePlugin);
  const replaceAndDeletePlugin = useAppStore((s) => s.replaceAndDeletePlugin);
  const removeReferencesAndDeletePlugin = useAppStore(
    (s) => s.removeReferencesAndDeletePlugin,
  );
  const forceDeletePluginInEditor = useAppStore(
    (s) => s.forceDeletePluginInEditor,
  );
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const isApplying = useAppStore((s) => s.isApplying);
  const isRestarting = useAppStore((s) => s.isRestarting);
  const configError = useAppStore((s) => s.configError);

  const [open, setOpen] = useState(false);
  const [preview, setPreview] = useState<PluginDeletePreview | null>(null);
  const [replacementTag, setReplacementTag] = useState("");
  const [actionError, setActionError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const busy = isConfigSaving || isApplying || isRestarting || loading;
  const disabled = busy || Boolean(configError);

  const openDialog = async () => {
    setOpen(true);
    setPreview(null);
    setActionError(null);
    setLoading(true);
    try {
      const nextPreview = await previewPluginDelete(plugin.id);
      setPreview(nextPreview);
      setReplacementTag(
        nextPreview.status === "ready"
          ? (nextPreview.replacementCandidates[0]?.name ?? "")
          : "",
      );
    } catch (error) {
      setPreview({
        status: "blocked",
        message: error instanceof Error ? error.message : "无法检查插件依赖",
      });
    } finally {
      setLoading(false);
    }
  };

  const runAction = async (action: () => Promise<void>) => {
    setActionError(null);
    setLoading(true);
    try {
      await action();
      setOpen(false);
      onDeleted?.();
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "删除失败");
    } finally {
      setLoading(false);
    }
  };

  const handleManualFix = () => {
    forceDeletePluginInEditor(plugin.id);
    setOpen(false);
    onDeleted?.();
  };

  const tooltip = configError
    ? "配置有错误，需先修复"
    : busy
      ? "配置操作进行中"
      : "删除";

  const readyPreview = preview?.status === "ready" ? preview : null;

  return (
    <>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant={variant}
            size={size}
            className={className}
            disabled={disabled}
            onClick={(event) => {
              if (stopPropagation) event.stopPropagation();
              if (!disabled) void openDialog();
            }}
          >
            <Trash2 className={cn("h-3.5 w-3.5", iconClassName)} />
            {label && <span>{label}</span>}
          </Button>
        </TooltipTrigger>
        <TooltipContent side="bottom">{tooltip}</TooltipContent>
      </Tooltip>

      <AlertDialog open={open} onOpenChange={setOpen}>
        <AlertDialogContent className="max-w-xl">
          <AlertDialogHeader>
            <AlertDialogTitle>
              {readyPreview && readyPreview.references.length > 0
                ? "该插件仍被引用"
                : "确认删除插件？"}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {readyPreview && readyPreview.references.length > 0
                ? "删除前需要先处理所有引用，否则配置会产生悬空依赖。"
                : `确定要删除插件 “${plugin.name}” 吗？此操作会先保存到磁盘，但不会自动应用。`}
            </AlertDialogDescription>
          </AlertDialogHeader>

          {loading && !preview ? (
            <p className="rounded-md border bg-muted/30 px-3 py-2 text-sm text-muted-foreground">
              正在检查依赖关系…
            </p>
          ) : preview?.status === "blocked" ? (
            <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {preview.message}
            </div>
          ) : readyPreview && readyPreview.references.length > 0 ? (
            <div className="space-y-4">
              <div className="max-h-52 space-y-2 overflow-auto rounded-md border bg-muted/20 p-2">
                {readyPreview.references.map((reference) => (
                  <div
                    key={`${reference.source_tag}:${reference.field}`}
                    className="rounded-md border bg-background px-3 py-2 text-xs"
                  >
                    <div className="flex flex-wrap items-center gap-1.5">
                      <span className="font-mono font-medium">
                        {reference.source_tag}
                      </span>
                      <Badge
                        variant="outline"
                        className="font-mono text-[10px]"
                      >
                        {reference.expected_kind}
                      </Badge>
                      {!reference.removable && (
                        <Badge variant="outline" className="text-[10px]">
                          需替换
                        </Badge>
                      )}
                    </div>
                    <div className="mt-1 font-mono text-muted-foreground">
                      {reference.field}
                    </div>
                    {reference.removeBlockedReason && (
                      <div className="mt-1 text-muted-foreground">
                        {reference.removeBlockedReason}
                      </div>
                    )}
                  </div>
                ))}
              </div>

              <div className="space-y-2">
                <div className="flex items-center gap-2 text-sm font-medium">
                  <ArrowRightLeft className="h-4 w-4 text-muted-foreground" />
                  替换引用并删除
                </div>
                <Select
                  value={replacementTag}
                  onValueChange={setReplacementTag}
                  disabled={
                    loading || readyPreview.replacementCandidates.length === 0
                  }
                >
                  <SelectTrigger>
                    <SelectValue placeholder="选择兼容的替换插件" />
                  </SelectTrigger>
                  <SelectContent>
                    {readyPreview.replacementCandidates.map((candidate) => (
                      <SelectItem key={candidate.id} value={candidate.name}>
                        {candidate.name} · {candidate.pluginKind}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {readyPreview.replacementCandidates.length === 0 && (
                  <p className="text-xs text-muted-foreground">
                    当前没有类型兼容的替换插件。
                  </p>
                )}
              </div>
            </div>
          ) : null}

          {actionError && (
            <div className="flex items-center gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              <AlertTriangle className="h-4 w-4" />
              {actionError}
            </div>
          )}

          <AlertDialogFooter className="gap-2 sm:gap-2">
            <AlertDialogCancel disabled={loading}>取消</AlertDialogCancel>
            {readyPreview && readyPreview.references.length === 0 && (
              <Button
                variant="destructive"
                disabled={loading}
                onClick={() => runAction(() => confirmDeletePlugin(plugin.id))}
              >
                删除并保存
              </Button>
            )}
            {readyPreview && readyPreview.references.length > 0 && (
              <>
                <Button
                  variant="outline"
                  disabled={loading}
                  onClick={handleManualFix}
                >
                  <Pencil className="mr-1.5 h-4 w-4" />
                  进入编辑器修复
                </Button>
                <Button
                  variant="outline"
                  disabled={loading || !readyPreview.canRemoveReferences}
                  onClick={() =>
                    runAction(() => removeReferencesAndDeletePlugin(plugin.id))
                  }
                >
                  移除引用并删除
                </Button>
                <Button
                  variant="destructive"
                  disabled={loading || !replacementTag}
                  onClick={() =>
                    runAction(() =>
                      replaceAndDeletePlugin(plugin.id, replacementTag),
                    )
                  }
                >
                  替换并删除
                </Button>
              </>
            )}
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
