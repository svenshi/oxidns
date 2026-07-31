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
  const explanation = plan?.plan.generated?.explanation;
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

            {diff?.affected ? (
              <div className="space-y-2 rounded-lg border p-3">
                <p className="font-medium">{t(WEBUI.standardApplyReview.semanticImpact)}</p>
                <div className="flex flex-wrap gap-2">
                  {Object.entries(diff.affected).flatMap(([category, values]) =>
                    values.map((value) => (
                      <Badge key={`${category}:${value}`} variant="outline">
                        {category}: {value}
                      </Badge>
                    )),
                  )}
                </div>
                {diff.summary?.map((item) => (
                  <p key={item} className="text-xs text-muted-foreground">{item}</p>
                ))}
              </div>
            ) : null}

            {explanation ? (
              <div className="space-y-3 rounded-lg border p-3">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <p className="font-medium">{t(WEBUI.standardApplyReview.compiledExplanation)}</p>
                  <code className="text-[11px] text-muted-foreground">
                    {explanation.intentRevision}
                  </code>
                </div>
                <div className="space-y-1">
                  {explanation.finalPriority.map((row) => (
                    <div key={`${row.category}:${row.stableId}`} className="flex flex-wrap gap-2 text-xs">
                      <Badge variant="secondary">#{row.ordinal} · slot {row.slot}</Badge>
                      <code>{row.category}:{row.stableId}</code>
                      <span className="text-muted-foreground">→ {row.selectedPathId ?? row.actionTag}</span>
                    </div>
                  ))}
                </div>
                <div className="grid gap-2 sm:grid-cols-2">
                  {explanation.pathBoundaries.map((path) => (
                    <div key={path.pathId} className="rounded-md bg-muted p-2 text-xs">
                      <p className="font-medium">{path.pathId} → {path.upstreamGroupId}</p>
                      <p className="text-muted-foreground">
                        cache {path.cacheEnabled ? path.cacheNamespace : t(WEBUI.standardApplyReview.cacheOff)} · ECS {path.ecsMode}
                        {path.ecsInKey ? t(WEBUI.standardApplyReview.isolatedKey) : ""}
                      </p>
                    </div>
                  ))}
                </div>
                {explanation.capabilities.missingOptional.length > 0 ? (
                  <p className="text-xs text-warning-foreground">
                    {t(WEBUI.standardApplyReview.missingOptional, { items: explanation.capabilities.missingOptional.join(", ") })}
                  </p>
                ) : null}
                <details>
                  <summary className="cursor-pointer text-xs font-medium">{t(WEBUI.standardApplyReview.yamlGraph)}</summary>
                  <pre className="mt-2 max-h-60 overflow-auto rounded-md bg-muted p-2 text-[10px]">
                    {plan.plan.generated?.yaml}
                    {"\n\n# dependency graph\n"}
                    {JSON.stringify(plan.dependency_graph ?? {}, null, 2)}
                  </pre>
                </details>
              </div>
            ) : null}

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
