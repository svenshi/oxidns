"use client";

import { useMemo, useState } from "react";
import dynamic from "next/dynamic";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Badge } from "@/components/ui/badge";
import {
  Plus,
  Server,
  Cog,
  Filter,
  Database,
  ArrowLeft,
  Search,
} from "lucide-react";
import { shouldPersistPluginMutation, useAppStore } from "@/lib/store";
import { extractOutboundProfileNames } from "@/lib/oxidns-config-schema";
import type { PluginType } from "@/lib/types";
import {
  getPluginCatalogItemsByType,
  getPluginKindIconComponent,
  type PluginCatalogItem,
} from "@/components/plugins/catalog";
import { LOCALES, WEBUI } from "@/lib/i18n";
import {
  getPluginSearchText,
  pluginTypeDescription,
  pluginTypeLabel,
} from "@/lib/i18n/plugin-defined";
import { useI18n } from "@/lib/i18n/provider";
import {
  createDefaultPluginConfigValues,
  isPluginConfigFormValid,
} from "@/components/plugins/plugin-config-fields-editor";
import { PluginConfigModeEditor } from "@/components/plugins/plugin-config-mode-editor";
import { isPluginKindSupported } from "@/lib/build-capabilities";
import {
  isReservedPluginTag,
  pluginTagValidationMessageKey,
  validatePluginTag,
} from "@/lib/plugin-tags";
import { cn } from "@/lib/utils";

const SequenceComposer = dynamic(
  () =>
    import("@/components/plugins/sequence-composer").then(
      (module) => module.SequenceComposer,
    ),
  { ssr: false },
);

const CronComposer = dynamic(
  () =>
    import("@/components/plugins/kinds/cron").then(
      (module) => module.CronComposer,
    ),
  { ssr: false },
);

const typeIcons: Record<PluginType, React.ReactNode> = {
  server: <Server className="h-4 w-4" />,
  executor: <Cog className="h-4 w-4" />,
  matcher: <Filter className="h-4 w-4" />,
  provider: <Database className="h-4 w-4" />,
};

interface CreatePluginDialogProps {
  defaultType?: PluginType;
  supportedTypes?: PluginType[];
  defaultName?: string;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  onCreated?: (tag: string) => void;
  trigger?: React.ReactNode;
  createButtonLabel?: string;
  title?: string;
  description?: string;
  supportedPluginKinds?: string[];
}

export function CreatePluginDialog({
  defaultType,
  supportedTypes,
  defaultName = "",
  open: controlledOpen,
  onOpenChange,
  onCreated,
  trigger,
  createButtonLabel,
  title,
  description,
  supportedPluginKinds,
}: CreatePluginDialogProps) {
  const { locale, t } = useI18n();
  const visibleTypes = useMemo(
    () =>
      supportedTypes?.length
        ? supportedTypes
        : (["server", "executor", "matcher", "provider"] as PluginType[]),
    [supportedTypes],
  );
  const resolvedCreateButtonLabel =
    createButtonLabel ?? t(WEBUI.plugins.create);
  const resolvedTitle = title ?? t(WEBUI.plugins.add);
  const resolvedDescription = description ?? t(WEBUI.plugins.addDescription);
  const initialType =
    defaultType && visibleTypes.includes(defaultType)
      ? defaultType
      : visibleTypes[0] || "server";
  const [uncontrolledOpen, setUncontrolledOpen] = useState(false);
  const open = controlledOpen ?? uncontrolledOpen;
  const setOpen = (nextOpen: boolean) => {
    if (controlledOpen === undefined) {
      setUncontrolledOpen(nextOpen);
    }
    onOpenChange?.(nextOpen);
  };
  const [activeTab, setActiveTab] = useState<PluginType>(initialType);
  const [selectedKind, setSelectedKind] = useState<PluginCatalogItem | null>(
    null,
  );
  const [instanceName, setInstanceName] = useState("");
  const [search, setSearch] = useState("");
  const [configValues, setConfigValues] = useState<Record<string, unknown>>({});
  const [configValid, setConfigValid] = useState(true);
  const addPlugin = useAppStore((s) => s.addPlugin);
  const saveConfig = useAppStore((s) => s.saveConfig);
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const plugins = useAppStore((s) => s.plugins);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const normalizedInstanceName = instanceName.trim();
  const instanceNameError = useMemo(() => {
    if (!normalizedInstanceName) return null;
    const validationError = validatePluginTag(normalizedInstanceName);
    if (validationError) {
      return t(pluginTagValidationMessageKey(validationError));
    }
    if (isReservedPluginTag(normalizedInstanceName)) {
      return t(WEBUI.storeErrors.pluginNameReserved);
    }
    if (plugins.some((plugin) => plugin.name === normalizedInstanceName)) {
      return t(WEBUI.storeErrors.pluginNameExists);
    }
    return null;
  }, [normalizedInstanceName, plugins, t]);
  const configModel = useAppStore((s) => s.configModel);
  const outboundProfileNames = useMemo(
    () => extractOutboundProfileNames(configModel),
    [configModel],
  );

  const pluginsByType = useMemo(() => {
    const supported = supportedPluginKinds?.length
      ? new Set(supportedPluginKinds)
      : null;
    const normalizedSearch = search.trim().toLowerCase();
    const byType = (type: PluginType) => {
      const plugins = getPluginCatalogItemsByType(type, locale);
      const supportedPlugins = supported
        ? plugins.filter((plugin) => supported.has(plugin.kind))
        : plugins;

      if (!normalizedSearch) return supportedPlugins;

      return supportedPlugins.filter((plugin) => {
        const configText = getConfigSearchText(plugin.configSchema);
        const searchableText = [
          plugin.kind,
          plugin.name,
          plugin.description,
          pluginTypeLabel(plugin.type, locale),
          configText,
          getPluginSearchText(plugin, [...LOCALES]),
        ]
          .filter(Boolean)
          .join(" ")
          .toLowerCase();

        return searchableText.includes(normalizedSearch);
      });
    };

    return {
      server: byType("server"),
      executor: byType("executor"),
      matcher: byType("matcher"),
      provider: byType("provider"),
    };
  }, [locale, search, supportedPluginKinds]);

  const handleSelectKind = (kind: PluginCatalogItem) => {
    if (!isPluginKindSupported(buildInfo, kind.type, kind.kind)) return;
    setSelectedKind(kind);
    setConfigValues(createDefaultPluginConfigValues(kind.configSchema));
    setConfigValid(true);
    setInstanceName(defaultName);
  };

  const handleBack = () => {
    setSelectedKind(null);
    setConfigValues({});
    setConfigValid(true);
    setInstanceName(defaultName);
  };

  const handleCreate = async () => {
    if (!selectedKind || !normalizedInstanceName || instanceNameError) return;
    if (!isPluginKindSupported(buildInfo, selectedKind.type, selectedKind.kind))
      return;

    const processedConfig = configValues;

    const tag = normalizedInstanceName;
    try {
      const resolution = await addPlugin({
        name: tag,
        type: selectedKind.type,
        pluginKind: selectedKind.kind,
        status: "stopped",
        enabled: false,
        pinned: false,
        config: processedConfig,
      });
      if (shouldPersistPluginMutation(resolution)) await saveConfig();
      if (resolution !== "cancelled") {
        onCreated?.(tag);
        handleClose();
      }
    } catch {
      // Store-level config error remains visible in the config editor.
    }
  };

  const handleClose = () => {
    setOpen(false);
    setSelectedKind(null);
    setConfigValues({});
    setConfigValid(true);
    setInstanceName(defaultName);
    setSearch("");
    setActiveTab(initialType);
  };

  const isValid = () => {
    if (!selectedKind || !normalizedInstanceName || instanceNameError)
      return false;
    if (!isPluginKindSupported(buildInfo, selectedKind.type, selectedKind.kind))
      return false;
    return (
      configValid &&
      isPluginConfigFormValid(selectedKind.configSchema, configValues)
    );
  };

  const renderPluginKindCard = (kind: PluginCatalogItem) => {
    const IconComponent = getPluginKindIconComponent(kind.icon);
    const supported = isPluginKindSupported(buildInfo, kind.type, kind.kind);
    return (
      <button
        key={kind.kind}
        type="button"
        disabled={!supported}
        title={supported ? undefined : t(WEBUI.common.unsupportedBuild)}
        onClick={() => handleSelectKind(kind)}
        className={cn(
          "flex w-full items-start gap-3 rounded-lg border border-border bg-card p-3 text-left transition-colors",
          supported
            ? "hover:border-primary/50 hover:bg-accent/50"
            : "cursor-not-allowed border-dashed opacity-55",
        )}
      >
        <div className="p-2 rounded-md bg-primary/10 text-primary shrink-0">
          <IconComponent className="h-4 w-4" />
        </div>
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-2">
            <div className="truncate text-sm font-medium">{kind.name}</div>
            {!supported && (
              <Badge variant="outline" className="shrink-0 text-[10px]">
                {t(WEBUI.common.notCompiled)}
              </Badge>
            )}
          </div>
          <div className="text-xs text-muted-foreground mt-0.5 line-clamp-2">
            {kind.description}
          </div>
        </div>
      </button>
    );
  };

  return (
    <Dialog
      onOpenChange={(isOpen) => {
        if (!isOpen && isSequenceFullscreenOpen()) return;
        if (!isOpen) handleClose();
        else {
          setActiveTab(initialType);
          setInstanceName(defaultName);
          setOpen(true);
        }
      }}
      open={open}
    >
      {trigger === null ? null : (
        <DialogTrigger asChild>
          {trigger ?? (
            <Button>
              <Plus className="h-4 w-4 mr-1.5" />
              {t(WEBUI.plugins.create)}
            </Button>
          )}
        </DialogTrigger>
      )}
      <DialogContent
        className="w-[calc(100vw-2rem)] sm:!max-w-[920px] lg:!max-w-[1080px] max-h-[90vh] p-4 gap-0 overflow-hidden"
        onPointerDownOutside={(event) => {
          if (isSequenceFullscreenEvent(event)) event.preventDefault();
        }}
        onInteractOutside={(event) => {
          if (isSequenceFullscreenEvent(event)) event.preventDefault();
        }}
      >
        {!selectedKind ? (
          <>
            <DialogHeader className="px-6 pt-6 pb-4">
              <DialogTitle>{resolvedTitle}</DialogTitle>
              <DialogDescription>{resolvedDescription}</DialogDescription>
            </DialogHeader>
            <Tabs
              value={activeTab}
              onValueChange={(v) => setActiveTab(v as PluginType)}
              className="flex-1"
            >
              <div className="px-6">
                <TabsList
                  className="grid w-full"
                  style={{
                    gridTemplateColumns: `repeat(${visibleTypes.length}, minmax(0, 1fr))`,
                  }}
                >
                  {visibleTypes.map((type) => (
                    <TabsTrigger key={type} value={type} className="gap-1.5">
                      {typeIcons[type]}
                      <span className="hidden sm:inline">
                        {pluginTypeLabel(type, locale)}
                      </span>
                    </TabsTrigger>
                  ))}
                </TabsList>
              </div>
              <div className="px-6 py-3">
                <div className="relative">
                  <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
                    placeholder={t(WEBUI.plugins.searchCatalogPlaceholder)}
                    className="pl-9"
                  />
                </div>
              </div>
              <div className="px-6 pb-2">
                <p className="text-xs text-muted-foreground">
                  {pluginTypeDescription(activeTab, locale)}
                </p>
              </div>
              <ScrollArea className="h-[min(560px,calc(90vh-180px))] px-6">
                {visibleTypes.map((type) => (
                  <TabsContent
                    key={type}
                    value={type}
                    className="mt-0 grid gap-3 pb-6 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
                  >
                    {pluginsByType[type].length > 0 ? (
                      pluginsByType[type].map(renderPluginKindCard)
                    ) : (
                      <div className="col-span-full rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
                        {t(WEBUI.plugins.noMatches)}
                      </div>
                    )}
                  </TabsContent>
                ))}
              </ScrollArea>
            </Tabs>
          </>
        ) : (
          <>
            <DialogHeader className="px-6 pt-6 pb-4 border-b">
              <div className="flex items-center gap-2">
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8"
                  onClick={handleBack}
                >
                  <ArrowLeft className="h-4 w-4" />
                </Button>
                <div>
                  <DialogTitle className="flex items-center gap-2">
                    {t(WEBUI.plugins.configureTitle, {
                      name: selectedKind.name,
                    })}
                    <Badge variant="secondary" className="font-normal">
                      {pluginTypeLabel(selectedKind.type, locale)}
                    </Badge>
                  </DialogTitle>
                  <DialogDescription className="mt-1">
                    {selectedKind.description}
                  </DialogDescription>
                </div>
              </div>
            </DialogHeader>
            <ScrollArea className="h-[min(600px,calc(90vh-180px))] px-5 py-4">
              <div className="px-1">
                <FieldGroup>
                  <Field>
                    <FieldLabel>
                      {t(WEBUI.plugins.instanceName)}{" "}
                      <span className="text-destructive">*</span>
                    </FieldLabel>
                    <Input
                      value={instanceName}
                      onChange={(e) => setInstanceName(e.target.value)}
                      placeholder={t(WEBUI.plugins.instanceNamePlaceholder, {
                        kind: selectedKind.kind,
                      })}
                      className="font-mono"
                      aria-invalid={Boolean(instanceNameError)}
                    />
                    <p
                      className={cn(
                        "mt-1 text-xs",
                        instanceNameError
                          ? "text-destructive"
                          : "text-muted-foreground",
                      )}
                    >
                      {instanceNameError ?? t(WEBUI.plugins.instanceNameHint)}
                    </p>
                  </Field>

                  <div className="border-t pt-4 mt-2">
                    <h4 className="text-sm font-medium mb-3">
                      {t(WEBUI.plugins.configTitle)}
                    </h4>
                    {selectedKind.kind === "sequence" ? (
                      <SequenceComposer
                        value={configValues}
                        onChange={setConfigValues}
                        plugins={plugins}
                        currentSequenceName={instanceName.trim() || undefined}
                        heightMode="dialog"
                      />
                    ) : selectedKind.kind === "cron" ? (
                      <CronComposer
                        value={configValues}
                        onChange={setConfigValues}
                        plugins={plugins}
                      />
                    ) : (
                      <PluginConfigModeEditor
                        key={selectedKind.kind}
                        fields={selectedKind.configSchema}
                        plugins={plugins}
                        values={configValues}
                        onChange={setConfigValues}
                        onValidityChange={setConfigValid}
                        pluginKind={selectedKind.kind}
                        currentPluginName={instanceName.trim() || undefined}
                        outboundProfileNames={outboundProfileNames}
                        advancedInitiallyConfigured={false}
                      />
                    )}
                  </div>
                </FieldGroup>
              </div>
            </ScrollArea>
            <DialogFooter className="px-6 py-4 border-t">
              <Button variant="outline" onClick={handleBack}>
                {t(WEBUI.common.back)}
              </Button>
              <Button
                onClick={handleCreate}
                disabled={!isValid() || isConfigSaving}
              >
                {isConfigSaving
                  ? t(WEBUI.common.saving)
                  : resolvedCreateButtonLabel}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

function getConfigSearchText(
  fields: PluginCatalogItem["configSchema"],
): string {
  return fields
    .map((field) =>
      [
        field.key,
        field.label,
        field.description,
        field.docs,
        field.options?.map((option) => option.label).join(" "),
        field.item ? getConfigChildSearchText(field.item) : undefined,
        field.itemOptions?.map(getConfigChildSearchText).join(" "),
        field.fields ? getConfigSearchText(field.fields) : undefined,
      ]
        .filter(Boolean)
        .join(" "),
    )
    .join(" ");
}

function getConfigChildSearchText(
  field: NonNullable<PluginCatalogItem["configSchema"][number]["item"]>,
): string {
  return [
    field.label,
    field.description,
    field.placeholder,
    "fields" in field ? getConfigSearchText(field.fields) : undefined,
    "item" in field && field.item
      ? getConfigChildSearchText(field.item)
      : undefined,
    "itemOptions" in field && field.itemOptions
      ? field.itemOptions.map(getConfigChildSearchText).join(" ")
      : undefined,
  ]
    .filter(Boolean)
    .join(" ");
}

function isSequenceFullscreenEvent(event: Event) {
  const target = event.target;
  return (
    target instanceof Element &&
    Boolean(target.closest("[data-sequence-fullscreen='true']"))
  );
}

function isSequenceFullscreenOpen() {
  return Boolean(document.querySelector("[data-sequence-fullscreen='true']"));
}
