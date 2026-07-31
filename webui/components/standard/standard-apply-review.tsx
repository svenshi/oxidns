"use client";

import { AlertTriangle, ShieldCheck } from "lucide-react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store";

export function StandardApplyReview() {
  const { t, formatNumber } = useI18n();
  const plan = useAppStore((state) => state.standardApplyConfirmation);
  const confirm = useAppStore((state) => state.confirmStandardApply);
  const cancel = useAppStore((state) => state.cancelStandardApply);
  const takeover = plan?.ownership !== "managed";
  const diff = plan?.semantic_diff;
  const diagnostics = plan?.plan.diagnostics ?? [];
  const ruleAnalysis = standardRuleAnalysis(plan?.plan.details.ruleAnalysis);
  const ownershipKey =
    plan?.ownership === "managed"
      ? WEBUI.standardApplyReview.ownershipManaged
      : plan?.ownership === "modified"
        ? WEBUI.standardApplyReview.ownershipModified
        : WEBUI.standardApplyReview.ownershipUnmanaged;

  return (
    <AlertDialog
      open={plan !== null}
      onOpenChange={(open) => {
        if (!open) cancel();
      }}
    >
      <AlertDialogContent size="wide">
        <AlertDialogHeader>
          <AlertDialogMedia
            className={
              takeover
                ? "bg-warning/15 text-warning-foreground"
                : "bg-primary/10 text-primary"
            }
          >
            {takeover ? <AlertTriangle /> : <ShieldCheck />}
          </AlertDialogMedia>
          <AlertDialogTitle>
            {t(WEBUI.standardApplyReview.title)}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {t(
              takeover
                ? WEBUI.standardApplyReview.takeoverDescription
                : WEBUI.standardApplyReview.managedDescription,
            )}
          </AlertDialogDescription>
        </AlertDialogHeader>

        {plan ? (
          <div className="max-h-[50vh] space-y-4 overflow-auto text-sm">
            <div className="flex items-center justify-between rounded-lg border p-3">
              <span className="text-muted-foreground">
                {t(WEBUI.standardApplyReview.ownership)}
              </span>
              <Badge variant={takeover ? "secondary" : "default"}>
                {t(ownershipKey)}
              </Badge>
            </div>

            <div className="grid gap-2 sm:grid-cols-3">
              <PlanCount
                text={t(WEBUI.standardApplyReview.generated, {
                  count: formatNumber(diff?.generated_plugin_tags.length ?? 0),
                })}
              />
              <PlanCount
                text={t(WEBUI.standardApplyReview.replaced, {
                  count: formatNumber(diff?.replaced_plugin_tags.length ?? 0),
                })}
              />
              <PlanCount
                destructive={(diff?.removed_plugin_tags.length ?? 0) > 0}
                text={t(WEBUI.standardApplyReview.removed, {
                  count: formatNumber(diff?.removed_plugin_tags.length ?? 0),
                })}
              />
            </div>

            <p className="rounded-lg bg-muted px-3 py-2 text-muted-foreground">
              {t(WEBUI.standardApplyReview.preserved, {
                items: diff?.preserved_top_level.join(", ") ?? "-",
              })}
            </p>

            {diagnostics.length > 0 ? (
              <div className="space-y-2">
                <p className="font-medium">
                  {t(WEBUI.standardApplyReview.diagnostics)}
                </p>
                <ul className="space-y-2">
                  {diagnostics.map((diagnostic) => (
                    <li
                      key={`${diagnostic.code}:${diagnostic.path}`}
                      className="rounded-lg border px-3 py-2"
                    >
                      <div className="flex items-center gap-2">
                        <Badge
                          variant={
                            diagnostic.severity === "error"
                              ? "destructive"
                              : "secondary"
                          }
                        >
                          {diagnostic.severity}
                        </Badge>
                        <code className="text-xs">{diagnostic.path}</code>
                      </div>
                      <p className="mt-1 text-muted-foreground">
                        {diagnostic.message}
                      </p>
                    </li>
                  ))}
                </ul>
              </div>
            ) : null}

            {ruleAnalysis.length > 0 ? (
              <div className="space-y-2">
                <p className="font-medium">
                  {t(WEBUI.standardApplyReview.ruleAnalysis)}
                </p>
                <ul className="space-y-2">
                  {ruleAnalysis.map((rule) => (
                    <li key={`${rule.category}:${rule.id}`} className="flex flex-wrap items-center gap-2 rounded-lg border px-3 py-2">
                      <Badge variant={rule.status === "effective" ? "secondary" : "outline"}>
                        {rule.status}
                      </Badge>
                      <code className="text-xs">{rule.category}:{rule.id}</code>
                      {rule.overriddenBy ? (
                        <span className="text-xs text-muted-foreground">
                          {t(WEBUI.standardApplyReview.overriddenBy, {
                            id: rule.overriddenBy,
                          })}
                        </span>
                      ) : null}
                    </li>
                  ))}
                </ul>
              </div>
            ) : null}
          </div>
        ) : null}

        <AlertDialogFooter>
          <AlertDialogCancel onClick={cancel}>
            {t(WEBUI.common.cancel)}
          </AlertDialogCancel>
          <AlertDialogAction
            variant={takeover ? "warning" : "default"}
            onClick={confirm}
          >
            {t(
              takeover
                ? WEBUI.standardApplyReview.takeover
                : WEBUI.standardApplyReview.apply,
            )}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

interface StandardRuleAnalysisRow {
  id: string;
  category: string;
  status: string;
  overriddenBy?: string;
}

function standardRuleAnalysis(value: unknown): StandardRuleAnalysisRow[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) => {
    if (!item || typeof item !== "object") return [];
    const row = item as Record<string, unknown>;
    if (
      typeof row.id !== "string" ||
      typeof row.category !== "string" ||
      typeof row.status !== "string"
    ) {
      return [];
    }
    return [{
      id: row.id,
      category: row.category,
      status: row.status,
      overriddenBy:
        typeof row.overriddenBy === "string" ? row.overriddenBy : undefined,
    }];
  });
}

function PlanCount({
  text,
  destructive = false,
}: {
  text: string;
  destructive?: boolean;
}) {
  return (
    <div
      className={
        destructive
          ? "rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-destructive"
          : "rounded-lg border p-3"
      }
    >
      {text}
    </div>
  );
}
