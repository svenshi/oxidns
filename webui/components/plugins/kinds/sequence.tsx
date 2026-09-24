/*
 * SPDX-FileCopyrightText: 2025 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { useState } from "react";
import { Pencil, Save } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { shouldPersistPluginMutation, useAppStore } from "@/lib/store";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import type {
  PluginComponentDefinition,
  PluginDetailComponentProps,
} from "@/components/plugins/types";
import { PluginDetailTemplate } from "@/components/plugins/plugin-detail-template";
import {
  SequenceComposer,
  parseSequenceRules,
} from "@/components/plugins/sequence-composer";

function SequenceDetail({
  plugin,
  chartData,
  onClose,
}: PluginDetailComponentProps) {
  const { t } = useI18n();
  const updatePluginConfig = useAppStore((state) => state.updatePluginConfig);
  const saveConfig = useAppStore((state) => state.saveConfig);
  const isConfigSaving = useAppStore((state) => state.isConfigSaving);
  const plugins = useAppStore((state) => state.plugins);
  const [editing, setEditing] = useState(false);
  const [configValues, setConfigValues] = useState<Record<string, unknown>>(
    () => plugin.config,
  );

  const handleCancel = () => {
    setConfigValues(plugin.config);
    setEditing(false);
  };

  const handleSave = async () => {
    try {
      const resolution = await updatePluginConfig(plugin.id, configValues);
      if (shouldPersistPluginMutation(resolution)) await saveConfig();
      if (resolution !== "cancelled") setEditing(false);
    } catch {
      // Store-level config errors are surfaced in the full config editor.
    }
  };

  return (
    <PluginDetailTemplate
      plugin={plugin}
      chartData={chartData}
      onClose={onClose}
      summaryItems={[
        {
          label: t(WEBUI.sequence.ruleCount),
          value: String(parseSequenceRules(plugin.config.args).length),
        },
      ]}
      configContent={
        <Card>
          <CardHeader className="grid grid-cols-[1fr_auto] items-center p-4 pb-2">
            <CardTitle className="text-sm">
              {t(WEBUI.sequence.arrangement)}
            </CardTitle>
            <div className="flex gap-2">
              {editing ? (
                <>
                  <Button variant="outline" size="sm" onClick={handleCancel}>
                    {t(WEBUI.common.cancel)}
                  </Button>
                  <Button
                    size="sm"
                    onClick={handleSave}
                    disabled={isConfigSaving}
                  >
                    <Save className="h-4 w-4" />
                    {isConfigSaving
                      ? t(WEBUI.sequence.saving)
                      : t(WEBUI.common.saveConfig)}
                  </Button>
                </>
              ) : (
                <Button size="sm" onClick={() => setEditing(true)}>
                  <Pencil className="h-4 w-4" />
                  {t(WEBUI.common.editConfig)}
                </Button>
              )}
            </div>
          </CardHeader>
          <CardContent className="p-4 pt-0">
            <SequenceComposer
              value={configValues}
              onChange={setConfigValues}
              plugins={plugins}
              readOnly={!editing}
              currentSequenceName={plugin.name}
              heightMode="detail"
              isSaving={isConfigSaving}
              onRequestEdit={() => setEditing(true)}
              onCancelEdit={handleCancel}
              onSaveEdit={handleSave}
            />
          </CardContent>
        </Card>
      }
    />
  );
}

export const sequencePlugin: PluginComponentDefinition = {
  Detail: SequenceDetail,
};
