/*
 * SPDX-FileCopyrightText: 2023 shadcn
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later AND MIT
 * Derived from shadcn/ui; see LICENSE-shadcn in the WebUI root.
 */

"use client";

import { cn } from "@/lib/utils";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import { Loader2Icon } from "lucide-react";

function Spinner({ className, ...props }: React.ComponentProps<"svg">) {
  const { t } = useI18n();
  return (
    <Loader2Icon
      role="status"
      aria-label={t(WEBUI.common.loading)}
      className={cn("size-4 animate-spin", className)}
      {...props}
    />
  );
}

export { Spinner };
