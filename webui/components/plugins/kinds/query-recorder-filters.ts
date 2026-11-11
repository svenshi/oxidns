/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import type { QueryRecordFilters } from "@/lib/oxidns-api";

export type TextMatchMode = "contains" | "exact";

export interface QueryRecordTextFilterForm {
  matcherTag: string;
  qname: string;
  qnameMatch: TextMatchMode;
  clientIp: string;
  clientIpMatch: TextMatchMode;
}

export function recordTextFilterForm(
  filters: QueryRecordFilters = {},
): QueryRecordTextFilterForm {
  return {
    matcherTag: filters.matcherTag ?? "",
    qname: filters.qnameExact ?? filters.qname ?? "",
    qnameMatch: filters.qnameExact ? "exact" : "contains",
    clientIp: filters.clientIpExact ?? filters.clientIp ?? "",
    clientIpMatch: filters.clientIpExact ? "exact" : "contains",
  };
}

export function recordTextFilters(
  form: QueryRecordTextFilterForm,
): QueryRecordFilters {
  const qname = form.qname.trim() || undefined;
  const clientIp = form.clientIp.trim() || undefined;
  // DNS question names are stored as absolute names. Accept input with or without a final dot.
  const exactQname = qname && !qname.endsWith(".") ? `${qname}.` : qname;
  return {
    matcherTag: form.matcherTag.trim() || undefined,
    qname: form.qnameMatch === "contains" ? qname : undefined,
    qnameExact: form.qnameMatch === "exact" ? exactQname : undefined,
    clientIp: form.clientIpMatch === "contains" ? clientIp : undefined,
    clientIpExact: form.clientIpMatch === "exact" ? clientIp : undefined,
  };
}
