/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import type { PluginDetailComponentProps } from "./types";
import { PluginDetailTemplate } from "./plugin-detail-template";

export function DefaultPluginDetail(props: PluginDetailComponentProps) {
  return <PluginDetailTemplate {...props} />;
}
