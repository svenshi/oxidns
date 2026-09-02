"use client";

import { AlertTriangle } from "lucide-react";
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
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store";

export function ConfigPatchConfirmationDialog() {
  const { t } = useI18n();
  const confirmation = useAppStore((state) => state.configPatchConfirmation);
  const resolve = useAppStore((state) => state.resolveConfigPatchConfirmation);

  return (
    <AlertDialog
      open={confirmation !== null}
      onOpenChange={(open) => {
        if (!open) resolve("cancel");
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogMedia className="bg-warning/10 text-warning">
            <AlertTriangle />
          </AlertDialogMedia>
          <AlertDialogTitle>{t(WEBUI.configPatch.title)}</AlertDialogTitle>
          <AlertDialogDescription className="space-y-2">
            <span className="block">{t(WEBUI.configPatch.description)}</span>
            {confirmation && (
              <code className="block break-all rounded-md bg-muted px-2 py-1.5 text-xs text-foreground">
                {t(WEBUI.configPatch.affectedPath, {
                  path: confirmation.affectedPath,
                })}
              </code>
            )}
            {confirmation && !confirmation.canForce && (
              <span className="block text-warning">
                {t(WEBUI.configPatch.forceUnavailable)}
              </span>
            )}
            {confirmation?.canForce && (
              <span className="block text-warning">
                {t(WEBUI.configPatch.forceWarning)}
              </span>
            )}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter className="sm:flex-wrap">
          <AlertDialogCancel onClick={() => resolve("cancel")}>
            {t(WEBUI.common.cancel)}
          </AlertDialogCancel>
          <AlertDialogAction
            variant="outline"
            onClick={() => resolve("review")}
          >
            {t(WEBUI.configPatch.review)}
          </AlertDialogAction>
          <AlertDialogAction
            variant="warning"
            disabled={!confirmation?.canForce}
            onClick={() => resolve("force")}
          >
            {t(WEBUI.configPatch.force)}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
