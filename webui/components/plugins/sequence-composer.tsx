/*
 * SPDX-FileCopyrightText: 2025 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  Background,
  Controls,
  Handle,
  Position,
  ReactFlow,
  useNodesState,
  type Edge,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import {
  ArrowDown,
  ArrowRight,
  GitBranch,
  GripHorizontal,
  Maximize2,
  Minimize2,
  Minus,
  Pencil,
  Plus,
  RotateCcw,
  Save,
  Trash2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { YamlEditor } from "@/components/config/yaml-editor";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { pluginTypeAccentHex } from "@/components/plugins/display";
import { CreatePluginDialog } from "@/components/plugins/create-plugin-dialog";
import { PluginReferencePicker } from "@/components/plugins/plugin-reference-picker";
import {
  InlineSelect,
  QuickSetupRow,
  createItemId,
  createStableItemId,
  firstQuickSetupKind,
  isQuickSetupValue,
  stripReferencePrefix,
} from "@/components/plugins/plugin-ref-editor";
import type { PluginInstance } from "@/lib/types";
import {
  parseArgsLevelPluginConfigYaml,
  stringifyArgsLevelPluginConfigYaml,
} from "@/lib/plugin-config-yaml";
import { cn } from "@/lib/utils";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";

type ConditionMode = "reference" | "quick_setup" | "text";
type ActionMode = "reference" | "quick_setup" | "control" | "text";
type ControlKind = "accept" | "return" | "reject" | "mark" | "jump" | "goto";

type SequenceFlowNode =
  | Node<RuleNodeData, "rule">
  | Node<PreviewNodeData, "preview">;

interface RuleNodeData extends Record<string, unknown> {
  // Content-derived storage key for persisted drag position. Stable across
  // rule reordering / insertion / deletion because it hashes the rule body
  // rather than its index. See `rulePositionKey` for the derivation.
  positionKey: string;
  rule: SequenceRule;
  index: number;
  total: number;
  plugins: PluginInstance[];
  sequenceTags: string[];
  readOnly: boolean;
  currentSequenceName?: string;
  visitedSequences: Set<string>;
  onChange: (patch: Partial<SequenceRule>) => void;
  onMove: (offset: number) => void;
  onDelete: () => void;
}

interface PreviewNodeData extends Record<string, unknown> {
  positionKey: string;
  action: SequenceAction;
  plugins: PluginInstance[];
  currentSequenceName?: string;
  visitedSequences: Set<string>;
}

interface SequenceCondition {
  id: string;
  mode: ConditionMode;
  value: string;
  invert: boolean;
}

interface SequenceAction {
  mode: ActionMode;
  value: string;
  control: ControlKind;
}

interface SequenceRule {
  id: string;
  matches: SequenceCondition[];
  action: SequenceAction;
}

type SequenceCanvasHeightMode = "inline" | "detail" | "dialog";

interface SequenceComposerProps {
  value: Record<string, unknown>;
  onChange: (value: Record<string, unknown>) => void;
  plugins: PluginInstance[];
  readOnly?: boolean;
  currentSequenceName?: string;
  heightMode?: SequenceCanvasHeightMode;
  isSaving?: boolean;
  onRequestEdit?: () => void;
  onCancelEdit?: () => void;
  onSaveEdit?: () => void | Promise<void>;
}

type NodePositions = Record<string, { x: number; y: number }>;

interface SequenceCanvasProps {
  rules: SequenceRule[];
  plugins: PluginInstance[];
  sequenceTags: string[];
  readOnly: boolean;
  currentSequenceName?: string;
  fullHeight?: boolean;
  heightMode?: SequenceCanvasHeightMode;
  savedPositions: NodePositions;
  onPositionChange: (nodeId: string, pos: { x: number; y: number }) => void;
  onResetPositions: () => void;
  onAddRule: () => void;
  onUpdateRule: (ruleId: string, patch: Partial<SequenceRule>) => void;
  onMoveRule: (index: number, offset: number) => void;
  onDeleteRule: (ruleId: string) => void;
}

const controlLabels: Record<ControlKind, string> = {
  accept: "accept",
  return: "return",
  reject: "reject",
  mark: "mark",
  jump: "jump",
  goto: "goto",
};

const builtinControls: ControlKind[] = [
  "accept",
  "return",
  "reject",
  "mark",
  "jump",
  "goto",
];

const flowNodeTypes = {
  rule: SequenceRuleFlowNode,
  preview: SequencePreviewFlowNode,
};

const sequenceNodeInteractionClass =
  "sequence-flow-interactive nodrag nopan nowheel";

export function SequenceComposer({
  value,
  onChange,
  plugins,
  readOnly = false,
  currentSequenceName,
  heightMode = "inline",
  isSaving = false,
  onRequestEdit,
  onCancelEdit,
  onSaveEdit,
}: SequenceComposerProps) {
  const { t } = useI18n();
  const [view, setView] = useState<"visual" | "yaml">("visual");
  const [expanded, setExpanded] = useState(false);
  const [yamlText, setYamlText] = useState(() =>
    stringifyArgsLevelPluginConfigYaml(value, true),
  );
  const [yamlError, setYamlError] = useState<string | null>(null);
  const rules = useMemo(() => parseSequenceRules(value.args), [value.args]);

  const positionStorageKey = `oxidns_seq_positions_${currentSequenceName ?? "_default"}`;
  const [savedPositions, setSavedPositions] = useState<NodePositions>(() => {
    try {
      return (
        (JSON.parse(
          localStorage.getItem(positionStorageKey) ?? "null",
        ) as NodePositions | null) ?? {}
      );
    } catch {
      return {};
    }
  });

  const handlePositionChange = (
    nodeId: string,
    pos: { x: number; y: number },
  ) => {
    setSavedPositions((prev) => {
      const next = { ...prev, [nodeId]: pos };
      localStorage.setItem(positionStorageKey, JSON.stringify(next));
      return next;
    });
  };

  const resetPositions = () => {
    setSavedPositions({});
    localStorage.removeItem(positionStorageKey);
  };

  const sequenceTags = useMemo(() => {
    const tags = new Set(
      plugins
        .filter(
          (plugin) =>
            plugin.type === "executor" && plugin.pluginKind === "sequence",
        )
        .map((plugin) => plugin.name),
    );
    if (currentSequenceName?.trim()) {
      tags.add(currentSequenceName.trim());
    }
    return Array.from(tags).sort((left, right) => left.localeCompare(right));
  }, [currentSequenceName, plugins]);

  const updateRules = (nextRules: SequenceRule[]) => {
    onChange({ ...value, args: serializeSequenceRules(nextRules) });
  };

  const addRule = () => {
    updateRules([...rules, createEmptyRule()]);
  };

  const updateRule = (ruleId: string, patch: Partial<SequenceRule>) => {
    updateRules(
      rules.map((rule) =>
        rule.id === ruleId
          ? {
              ...rule,
              ...patch,
            }
          : rule,
      ),
    );
  };

  const moveRule = (index: number, offset: number) => {
    const nextIndex = index + offset;
    if (nextIndex < 0 || nextIndex >= rules.length) return;
    const nextRules = [...rules];
    const [rule] = nextRules.splice(index, 1);
    nextRules.splice(nextIndex, 0, rule);
    updateRules(nextRules);
  };

  const deleteRule = (ruleId: string) => {
    updateRules(rules.filter((rule) => rule.id !== ruleId));
  };

  const handleViewChange = (nextView: "visual" | "yaml") => {
    if (nextView === "yaml") {
      setYamlText(stringifyArgsLevelPluginConfigYaml(value, true));
      setYamlError(null);
    }
    setView(nextView);
  };

  const handleYamlChange = (nextYaml: string) => {
    setYamlText(nextYaml);
    if (readOnly) return;

    const parsed = parseArgsLevelPluginConfigYaml(nextYaml, true);
    if (parsed.error) {
      setYamlError(parsed.error);
      return;
    }

    if (
      parsed.value &&
      typeof parsed.value === "object" &&
      !Array.isArray(parsed.value)
    ) {
      setYamlError(null);
      onChange(parsed.value as Record<string, unknown>);
      return;
    }

    setYamlError(t(WEBUI.sequence.yamlMustBeObject));
  };

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Tabs
          value={view}
          onValueChange={(next) => handleViewChange(next as typeof view)}
        >
          <TabsList className="grid w-44 max-w-full grid-cols-2">
            <TabsTrigger value="visual">
              {t(WEBUI.sequence.canvasTab)}
            </TabsTrigger>
            <TabsTrigger value="yaml">YAML</TabsTrigger>
          </TabsList>
        </Tabs>
        {view === "yaml" && yamlError && (
          <Badge
            variant="destructive"
            className="h-auto gap-1 whitespace-normal py-1"
          >
            {yamlError}
          </Badge>
        )}
        {!readOnly && view === "visual" && (
          <div className="flex flex-wrap items-center gap-2">
            <CreateDependencyPluginButton />
            {Object.keys(savedPositions).length > 0 && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={resetPositions}
              >
                <RotateCcw className="h-4 w-4" />
                {t(WEBUI.sequence.resetLayout)}
              </Button>
            )}
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => setExpanded(true)}
            >
              <Maximize2 className="h-4 w-4" />
              {t(WEBUI.sequence.fullscreen)}
            </Button>
            <Button type="button" size="sm" onClick={addRule}>
              <Plus className="h-4 w-4" />
              {t(WEBUI.sequence.addRule)}
            </Button>
          </div>
        )}
        {readOnly && view === "visual" && (
          <div className="flex items-center gap-2">
            {Object.keys(savedPositions).length > 0 && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={resetPositions}
              >
                <RotateCcw className="h-4 w-4" />
                {t(WEBUI.sequence.resetLayout)}
              </Button>
            )}
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => setExpanded(true)}
            >
              <Maximize2 className="h-4 w-4" />
              {t(WEBUI.sequence.fullscreen)}
            </Button>
          </div>
        )}
      </div>

      {view === "visual" && (
        <>
          <SequenceCanvas
            rules={rules}
            plugins={plugins}
            sequenceTags={sequenceTags}
            readOnly={readOnly}
            currentSequenceName={currentSequenceName}
            heightMode={heightMode}
            savedPositions={savedPositions}
            onPositionChange={handlePositionChange}
            onResetPositions={resetPositions}
            onAddRule={addRule}
            onUpdateRule={updateRule}
            onMoveRule={moveRule}
            onDeleteRule={deleteRule}
          />
          {expanded && (
            <SequenceExpandedCanvas
              rules={rules}
              plugins={plugins}
              sequenceTags={sequenceTags}
              readOnly={readOnly}
              currentSequenceName={currentSequenceName}
              onClose={() => setExpanded(false)}
              isSaving={isSaving}
              onRequestEdit={onRequestEdit}
              onCancelEdit={onCancelEdit}
              onSaveEdit={onSaveEdit}
              savedPositions={savedPositions}
              onPositionChange={handlePositionChange}
              onResetPositions={resetPositions}
              onAddRule={addRule}
              onUpdateRule={updateRule}
              onMoveRule={moveRule}
              onDeleteRule={deleteRule}
            />
          )}
        </>
      )}

      {view === "yaml" && (
        <YamlEditor
          value={yamlText}
          onChange={handleYamlChange}
          readOnly={readOnly}
          className="min-h-[260px]"
          variant="sequence"
          plugins={plugins}
          pluginKind="sequence"
          currentPluginName={currentSequenceName}
        />
      )}
    </div>
  );
}

function SequenceExpandedCanvas({
  rules,
  plugins,
  sequenceTags,
  readOnly,
  currentSequenceName,
  onClose,
  isSaving,
  onRequestEdit,
  onCancelEdit,
  onSaveEdit,
  savedPositions,
  onPositionChange,
  onResetPositions,
  onAddRule,
  onUpdateRule,
  onMoveRule,
  onDeleteRule,
}: {
  rules: SequenceRule[];
  plugins: PluginInstance[];
  sequenceTags: string[];
  readOnly: boolean;
  currentSequenceName?: string;
  onClose: () => void;
  isSaving: boolean;
  onRequestEdit?: () => void;
  onCancelEdit?: () => void;
  onSaveEdit?: () => void | Promise<void>;
  savedPositions: NodePositions;
  onPositionChange: (nodeId: string, pos: { x: number; y: number }) => void;
  onResetPositions: () => void;
  onAddRule: () => void;
  onUpdateRule: (ruleId: string, patch: Partial<SequenceRule>) => void;
  onMoveRule: (index: number, offset: number) => void;
  onDeleteRule: (ruleId: string) => void;
}) {
  const { t } = useI18n();
  return (
    <div
      data-sequence-fullscreen="true"
      className="pointer-events-auto fixed inset-0 z-[1000] flex h-dvh w-screen flex-col overflow-hidden bg-background"
      onPointerDown={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          onClose();
        }
      }}
    >
      <div className="flex min-h-14 items-center justify-between gap-3 border-b bg-sidebar/80 px-4">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-sm font-medium">
            <GitBranch className="h-4 w-4 text-primary" />
            <span>{t(WEBUI.sequence.canvasTitle)}</span>
            <Badge variant="secondary" className="font-mono">
              {rules.length} rules
            </Badge>
          </div>
          <div className="mt-0.5 text-xs text-muted-foreground">
            {readOnly
              ? t(WEBUI.sequence.viewModeDesc)
              : t(WEBUI.sequence.editModeDesc)}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {readOnly ? (
            onRequestEdit && (
              <Button type="button" size="sm" onClick={onRequestEdit}>
                <Pencil className="h-4 w-4" />
                {t(WEBUI.sequence.editMode)}
              </Button>
            )
          ) : (
            <>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={onAddRule}
              >
                <Plus className="h-4 w-4" />
                {t(WEBUI.sequence.addRule)}
              </Button>
              {onCancelEdit && (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={onCancelEdit}
                >
                  {t(WEBUI.common.cancel)}
                </Button>
              )}
              {onSaveEdit && (
                <Button
                  type="button"
                  size="sm"
                  onClick={() => void onSaveEdit()}
                  disabled={isSaving}
                >
                  <Save className="h-4 w-4" />
                  {isSaving
                    ? t(WEBUI.sequence.saving)
                    : t(WEBUI.common.saveConfig)}
                </Button>
              )}
            </>
          )}
          {Object.keys(savedPositions).length > 0 && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={onResetPositions}
            >
              <RotateCcw className="h-4 w-4" />
              {t(WEBUI.sequence.resetLayout)}
            </Button>
          )}
          <Button type="button" variant="outline" size="sm" onClick={onClose}>
            <Minimize2 className="h-4 w-4" />
            {t(WEBUI.sequence.exitFullscreen)}
          </Button>
        </div>
      </div>
      <div className="min-h-0 flex-1 p-4">
        <SequenceCanvas
          rules={rules}
          plugins={plugins}
          sequenceTags={sequenceTags}
          readOnly={readOnly}
          currentSequenceName={currentSequenceName}
          fullHeight
          savedPositions={savedPositions}
          onPositionChange={onPositionChange}
          onResetPositions={onResetPositions}
          onAddRule={onAddRule}
          onUpdateRule={onUpdateRule}
          onMoveRule={onMoveRule}
          onDeleteRule={onDeleteRule}
        />
      </div>
    </div>
  );
}

function SequenceEmptyState({
  readOnly,
  heightMode = "inline",
  fullHeight = false,
  onAddRule,
}: {
  readOnly: boolean;
  heightMode?: SequenceCanvasHeightMode;
  fullHeight?: boolean;
  onAddRule: () => void;
}) {
  const { t } = useI18n();
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center rounded-lg border border-dashed p-8 text-center",
        getSequenceCanvasHeightClass(heightMode, fullHeight),
      )}
    >
      <GitBranch className="mx-auto h-8 w-8 text-muted-foreground" />
      <div className="mt-3 text-sm font-medium">
        {t(WEBUI.sequence.noRules)}
      </div>
      <p className="mt-1 text-xs text-muted-foreground">
        {t(WEBUI.sequence.noRulesDesc)}
      </p>
      {!readOnly && (
        <Button type="button" className="mt-4" onClick={onAddRule}>
          <Plus className="h-4 w-4" />
          {t(WEBUI.sequence.addFirstRule)}
        </Button>
      )}
    </div>
  );
}

function SequenceCanvas({
  rules,
  plugins,
  sequenceTags,
  readOnly,
  currentSequenceName,
  fullHeight = false,
  heightMode = "inline",
  savedPositions,
  onPositionChange,
  onUpdateRule,
  onMoveRule,
  onDeleteRule,
  onAddRule,
}: SequenceCanvasProps) {
  // Derive flow nodes/edges from props. Memoised on the actual data inputs so
  // the reference is stable across renders that don't affect graph contents.
  const built = useMemo(
    () =>
      buildSequenceFlow({
        rules,
        plugins,
        sequenceTags,
        readOnly,
        currentSequenceName,
        savedPositions,
        onUpdateRule,
        onMoveRule,
        onDeleteRule,
      }),
    [
      rules,
      plugins,
      sequenceTags,
      readOnly,
      currentSequenceName,
      savedPositions,
      onUpdateRule,
      onMoveRule,
      onDeleteRule,
    ],
  );

  // Hold nodes as local state so React Flow's d3-drag updates land via
  // `onNodesChange` and re-render the node DURING the gesture. Without this
  // the node only "jumps" to its new spot on drag-stop, because the position
  // prop never reflects the in-progress drag.
  const [nodes, setNodes, onNodesChange] = useNodesState<SequenceFlowNode>(
    built.nodes,
  );

  // Re-sync when rules / savedPositions / readOnly change. `built.nodes` is
  // referentially stable thanks to the memo above, so this only fires on real
  // input changes — never mid-drag.
  useEffect(() => {
    setNodes(built.nodes);
  }, [built.nodes, setNodes]);

  if (rules.length === 0) {
    return (
      <SequenceEmptyState
        readOnly={readOnly}
        heightMode={heightMode}
        fullHeight={fullHeight}
        onAddRule={onAddRule}
      />
    );
  }

  const hasCustomPositions = Object.keys(savedPositions).length > 0;

  return (
    <div
      className={cn(
        "sequence-flow min-h-0 rounded-lg border bg-muted/20",
        getSequenceCanvasHeightClass(heightMode, fullHeight),
      )}
    >
      <ReactFlow<SequenceFlowNode, Edge>
        nodes={nodes}
        edges={built.edges}
        onNodesChange={onNodesChange}
        nodeTypes={flowNodeTypes}
        fitView={!hasCustomPositions}
        fitViewOptions={{ padding: 0.16 }}
        minZoom={0.35}
        maxZoom={1.8}
        nodesDraggable
        nodesConnectable={false}
        nodesFocusable={false}
        edgesFocusable={false}
        elementsSelectable={false}
        deleteKeyCode={null}
        selectionKeyCode={null}
        multiSelectionKeyCode={null}
        panActivationKeyCode={null}
        zoomActivationKeyCode={null}
        disableKeyboardA11y
        // Use the default `nodrag` / `nopan` / `nowheel` classes. React Flow's
        // NodeWrapper applies `noPanClassName` to the node container itself, so
        // sharing a single class for all three would put `nodrag` on the node
        // root and block drags from the grip handle. The `InteractiveNodeFrame`
        // already carries `nodrag nopan nowheel` to protect controls inside.
        panOnDrag={[0]}
        zoomOnScroll
        zoomOnPinch
        zoomOnDoubleClick={false}
        preventScrolling
        onNodeDragStop={(_event, node) => {
          // Persist by the content-derived storage key carried in node.data,
          // not the React Flow id. That way moving / deleting rules around it
          // doesn't drag this rule's saved position onto a different rule.
          const key = (node.data as { positionKey?: string }).positionKey;
          if (key) onPositionChange(key, node.position);
        }}
      >
        <Background gap={18} size={1} />
        <Controls showInteractive={false} />
      </ReactFlow>
    </div>
  );
}

function getSequenceCanvasHeightClass(
  mode: SequenceCanvasHeightMode,
  fullHeight: boolean,
) {
  if (fullHeight) return "h-full";

  switch (mode) {
    case "dialog":
      return "h-[clamp(240px,30dvh,320px)]";
    case "detail":
      return "h-[clamp(360px,52dvh,620px)]";
    case "inline":
    default:
      return "h-[clamp(340px,50dvh,560px)]";
  }
}

function buildSequenceFlow({
  rules,
  plugins,
  sequenceTags,
  readOnly,
  currentSequenceName,
  savedPositions,
  onUpdateRule,
  onMoveRule,
  onDeleteRule,
}: {
  rules: SequenceRule[];
  plugins: PluginInstance[];
  sequenceTags: string[];
  readOnly: boolean;
  currentSequenceName?: string;
  savedPositions: NodePositions;
  onUpdateRule: (ruleId: string, patch: Partial<SequenceRule>) => void;
  onMoveRule: (index: number, offset: number) => void;
  onDeleteRule: (ruleId: string) => void;
}): { nodes: SequenceFlowNode[]; edges: Edge[] } {
  const nodes: SequenceFlowNode[] = [];
  const edges: Edge[] = [];
  const baseVisited = currentSequenceName
    ? new Set([currentSequenceName])
    : new Set<string>();
  let currentY = 0;
  // Tracks the bottom of the last placed preview node so previews never overlap.
  let rightColumnBottom = 0;
  // Rule nodes are 1200px wide; leave a 140px gap before the preview column.
  const PREVIEW_X = 1340;

  // Disambiguate identical-content rules: first occurrence gets the bare
  // fingerprint, subsequent ones get `#1`, `#2`, …  This keeps duplicate
  // rules visually pinnable while still letting move/delete carry positions.
  const keyOccurrences = new Map<string, number>();

  rules.forEach((rule, index) => {
    const ruleId = `rule-${rule.id}`;
    const baseKey = rulePositionKey(rule);
    const occ = keyOccurrences.get(baseKey) ?? 0;
    keyOccurrences.set(baseKey, occ + 1);
    const positionKey =
      occ === 0 ? `rule:${baseKey}` : `rule:${baseKey}#${occ}`;
    const ruleY = currentY;
    nodes.push({
      id: ruleId,
      type: "rule",
      position: savedPositions[positionKey] ?? { x: 0, y: ruleY },
      dragHandle: ".sequence-drag-handle",
      data: {
        positionKey,
        rule,
        index,
        total: rules.length,
        plugins,
        sequenceTags,
        readOnly,
        currentSequenceName,
        visitedSequences: baseVisited,
        onChange: (patch) => onUpdateRule(rule.id, patch),
        onMove: (offset) => onMoveRule(index, offset),
        onDelete: () => onDeleteRule(rule.id),
      },
      selectable: false,
      focusable: false,
    });

    if (index < rules.length - 1) {
      // Pin the edge to the bottom "next" handle and the next rule's top
      // target. Without explicit handle ids the source rule has two source
      // handles (bottom + right), so React Flow can't tell which one to
      // anchor on — once the node is dragged, the line routes randomly.
      edges.push({
        id: `seq-${rule.id}-${rules[index + 1].id}`,
        source: ruleId,
        sourceHandle: "next",
        target: `rule-${rules[index + 1].id}`,
        type: "smoothstep",
        animated: false,
        style: { strokeWidth: 2 },
      });
    }

    const target = getSequenceControlTarget(rule.action);
    if (target) {
      const previewId = `preview-${rule.id}-${target}`;
      const previewKey = `preview:${positionKey}:${target}`;
      // Align with the rule's Y when possible, but push down if a previous
      // preview occupies that vertical space.
      const previewY = Math.max(ruleY, rightColumnBottom);
      nodes.push({
        id: previewId,
        type: "preview",
        position: savedPositions[previewKey] ?? { x: PREVIEW_X, y: previewY },
        dragHandle: ".sequence-drag-handle",
        data: {
          positionKey: previewKey,
          action: rule.action,
          plugins,
          currentSequenceName,
          visitedSequences: baseVisited,
        },
        selectable: false,
        focusable: false,
      });
      rightColumnBottom =
        previewY + estimatePreviewHeight(target, plugins) + 40;
      // Colour the branch edge to match the dependency-graph palette so the
      // canvas reads as the same system: forward references / jumps use sky
      // (executor accent), goto uses red dashed/animated like a back-edge.
      const isGoto = rule.action.control === "goto";
      edges.push({
        id: `branch-${rule.id}-${target}`,
        source: ruleId,
        sourceHandle: "branch",
        target: previewId,
        type: "smoothstep",
        animated: isGoto,
        style: {
          stroke: isGoto ? "#ef4444" : pluginTypeAccentHex.executor,
          strokeWidth: 2,
          strokeDasharray: isGoto ? "6 3" : undefined,
        },
      });
    }

    currentY += estimateRuleNodeHeight(rule) + 40;
  });

  return { nodes, edges };
}

function estimateRuleNodeHeight(rule: SequenceRule) {
  const baseHeight = 176;
  const conditionCount = Math.max(rule.matches.length, 1);
  const extraConditions = Math.max(conditionCount - 1, 0);
  return baseHeight + extraConditions * 56;
}

function estimatePreviewHeight(
  targetTag: string,
  plugins: PluginInstance[],
): number {
  const targetPlugin = plugins.find(
    (p) =>
      p.name === targetTag &&
      p.type === "executor" &&
      p.pluginKind === "sequence",
  );
  if (!targetPlugin) return 100;
  const targetRules = parseSequenceRules(targetPlugin.config.args);
  if (targetRules.length === 0) return 100;
  const rulesHeight = targetRules.reduce(
    (sum, r) => sum + estimateRuleNodeHeight(r),
    0,
  );
  // 60px: outer padding (24px) + header row (36px). 12px gap between sub-rules.
  return 60 + rulesHeight + Math.max(targetRules.length - 1, 0) * 12;
}

function SequenceRuleFlowNode({ data }: NodeProps<Node<RuleNodeData, "rule">>) {
  return (
    <>
      <Handle type="target" position={Position.Top} />
      {/* Drag handle — must be OUTSIDE InteractiveNodeFrame so pointer events
          reach ReactFlow. InteractiveNodeFrame stops all propagation to protect
          the interactive controls inside, so any draggable area must be a
          sibling, not a descendant. */}
      <SequenceNodeDragHandle />
      <InteractiveNodeFrame>
        <SequenceRuleNode {...data} />
      </InteractiveNodeFrame>
      <Handle type="source" position={Position.Bottom} id="next" />
      <Handle type="source" position={Position.Right} id="branch" />
    </>
  );
}

function SequencePreviewFlowNode({
  data,
}: NodeProps<Node<PreviewNodeData, "preview">>) {
  return (
    <>
      <Handle type="target" position={Position.Left} />
      <SequenceNodeDragHandle />
      <InteractiveNodeFrame>
        <SequenceReferencePreview {...data} />
      </InteractiveNodeFrame>
    </>
  );
}

function SequenceNodeDragHandle() {
  return (
    <div className="sequence-drag-handle flex h-5 cursor-grab items-center justify-center rounded-t-md border border-b-0 bg-muted/30 active:cursor-grabbing">
      <GripHorizontal className="h-3.5 w-3.5 text-muted-foreground/40" />
    </div>
  );
}

function InteractiveNodeFrame({ children }: { children: ReactNode }) {
  return (
    <div
      className={sequenceNodeInteractionClass}
      onPointerDown={(event) => event.stopPropagation()}
      onMouseDown={(event) => event.stopPropagation()}
      onTouchStart={(event) => event.stopPropagation()}
      onWheel={(event) => event.stopPropagation()}
      onKeyDown={(event) => event.stopPropagation()}
      onKeyUp={(event) => event.stopPropagation()}
    >
      {children}
    </div>
  );
}

function SequenceRuleNode({
  rule,
  index,
  total,
  plugins,
  sequenceTags,
  readOnly,
  onChange,
  onMove,
  onDelete,
}: {
  rule: SequenceRule;
  index: number;
  total: number;
  plugins: PluginInstance[];
  sequenceTags: string[];
  readOnly: boolean;
  onChange: (patch: Partial<SequenceRule>) => void;
  onMove: (offset: number) => void;
  onDelete: () => void;
}) {
  const addCondition = () => {
    onChange({ matches: [...rule.matches, createEmptyCondition()] });
  };

  const updateCondition = (
    conditionId: string,
    patch: Partial<SequenceCondition>,
  ) => {
    onChange({
      matches: rule.matches.map((condition) =>
        condition.id === conditionId ? { ...condition, ...patch } : condition,
      ),
    });
  };

  const deleteCondition = (conditionId: string) => {
    onChange({
      matches: rule.matches.filter((condition) => condition.id !== conditionId),
    });
  };

  const { t } = useI18n();
  const branchTarget = getSequenceControlTarget(rule.action);
  const hasBranch = Boolean(branchTarget);

  return (
    <div className={sequenceNodeInteractionClass}>
      <Card className="w-[1200px] max-w-[96vw] rounded-lg bg-background py-0 shadow-sm">
        <CardHeader className="grid grid-cols-[1fr_auto] items-center gap-2 border-b px-3 py-2">
          <div className="flex min-w-0 items-center gap-2">
            <Badge
              variant="secondary"
              className={cn(
                "font-mono",
                hasBranch &&
                  "bg-sky-100 text-sky-700 dark:bg-sky-950 dark:text-sky-200",
              )}
            >
              #{index + 1}
            </Badge>
            <CardTitle className="truncate text-sm">
              {summarizeRule(rule, t)}
            </CardTitle>
          </div>
          {!readOnly && (
            <div className="flex items-center gap-1">
              <Button
                type="button"
                variant="outline"
                size="icon-sm"
                disabled={index === 0}
                onClick={() => onMove(-1)}
                aria-label={t(WEBUI.sequence.moveUp)}
              >
                <ArrowDown className="h-4 w-4 rotate-180" />
              </Button>
              <Button
                type="button"
                variant="outline"
                size="icon-sm"
                disabled={index === total - 1}
                onClick={() => onMove(1)}
                aria-label={t(WEBUI.sequence.moveDown)}
              >
                <ArrowDown className="h-4 w-4" />
              </Button>
              <Button
                type="button"
                variant="outline"
                size="icon-sm"
                onClick={onDelete}
                aria-label={t(WEBUI.sequence.deleteRule)}
              >
                <Trash2 className="h-4 w-4" />
              </Button>
            </div>
          )}
        </CardHeader>
        <CardContent className="grid items-stretch gap-4 p-3 lg:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)]">
          <div className="space-y-2">
            <div className="flex h-6 items-center justify-between gap-2">
              <div className="text-xs font-semibold uppercase tracking-wide text-amber-700 dark:text-amber-300">
                {t(WEBUI.sequence.ifCondition)}
              </div>
              {!readOnly && (
                <Button
                  type="button"
                  variant="outline"
                  size="xs"
                  onClick={addCondition}
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t(WEBUI.sequence.condition)}
                </Button>
              )}
            </div>
            {rule.matches.length > 0 ? (
              <div className="space-y-2">
                {rule.matches.map((condition) => (
                  <ConditionEditor
                    key={condition.id}
                    condition={condition}
                    plugins={plugins}
                    readOnly={readOnly}
                    onChange={(patch) => updateCondition(condition.id, patch)}
                    onDelete={() => deleteCondition(condition.id)}
                  />
                ))}
              </div>
            ) : (
              <div className="rounded-md border border-dashed border-amber-300/60 bg-amber-50/30 px-3 py-4 text-center text-xs italic text-muted-foreground dark:border-amber-800/40 dark:bg-amber-950/15">
                {t(WEBUI.sequence.unconditional)}
              </div>
            )}
          </div>

          <div
            className="hidden h-full flex-col justify-center px-1 lg:flex"
            style={{ color: pluginTypeAccentHex.executor }}
          >
            <div className="h-6" />
            <div className="mt-2 flex min-h-12 items-center">
              <ArrowRight className="h-5 w-5" />
            </div>
          </div>

          <div className="flex h-full flex-col justify-center space-y-2">
            <div className="flex h-6 items-center">
              <div className="text-xs font-semibold uppercase tracking-wide text-sky-700 dark:text-sky-300">
                {t(WEBUI.sequence.thenAction)}
              </div>
            </div>
            <ActionEditor
              action={rule.action}
              plugins={plugins}
              sequenceTags={sequenceTags}
              readOnly={readOnly}
              onChange={(action) => onChange({ action })}
            />
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

function ConditionEditor({
  condition,
  plugins,
  readOnly,
  onChange,
  onDelete,
}: {
  condition: SequenceCondition;
  plugins: PluginInstance[];
  readOnly: boolean;
  onChange: (patch: Partial<SequenceCondition>) => void;
  onDelete: () => void;
}) {
  // localMode is authoritative for the dropdown — it is NOT reset by YAML
  // round-trips that re-classify condition.mode. The component is keyed by
  // condition.id so useState initialises fresh whenever a different condition
  // is displayed.
  const { t } = useI18n();
  const [localMode, setLocalMode] = useState<ConditionMode>(condition.mode);

  const handleModeChange = (mode: ConditionMode) => {
    if (mode === localMode) return;
    setLocalMode(mode);
    if (mode === "text") {
      // Text mode is a raw-edit lens over the current serialised value.
      // No value reset needed — just change the display mode locally.
      return;
    }
    if (mode === "reference") {
      const tag = stripReferencePrefix(condition.value);
      onChange({ mode, value: tag ? `$${tag}` : "$has_resp" });
      return;
    }
    onChange({ mode, value: defaultConditionValue(mode) });
  };

  return (
    <div className="rounded-md border border-amber-200/80 bg-amber-50/40 px-2 py-1.5 dark:border-amber-800/40 dark:bg-amber-950/20">
      <div className="flex min-w-0 items-center gap-1.5">
        <InlineSelect
          value={localMode}
          onChange={(mode) => handleModeChange(mode as ConditionMode)}
          disabled={readOnly}
          className="w-[4.5rem] shrink-0"
          options={[
            { value: "reference", label: t(WEBUI.sequence.modeReference) },
            { value: "quick_setup", label: t(WEBUI.sequence.modeQuickSetup) },
            { value: "text", label: t(WEBUI.sequence.modeText) },
          ]}
        />
        {(localMode === "reference" || localMode === "quick_setup") && (
          <InvertCheckbox
            checked={condition.invert}
            disabled={readOnly}
            onCheckedChange={(invert) => onChange({ invert })}
          />
        )}
        <div className="min-w-0 flex-1">
          {localMode === "reference" ? (
            <PluginReferencePicker
              plugins={plugins}
              value={stripReferencePrefix(condition.value)}
              referenceTypes={["matcher"]}
              disabled={readOnly}
              placeholder={t(WEBUI.sequence.selectMatcher)}
              className="h-8 min-h-8 py-0"
              allowCreate
              onChange={(tag) => onChange({ value: `$${tag}` })}
            />
          ) : localMode === "quick_setup" ? (
            <QuickSetupRow
              type="matcher"
              value={condition.value}
              plugins={plugins}
              readOnly={readOnly}
              onChange={(next) => onChange({ value: next })}
            />
          ) : (
            <Input
              value={condition.value}
              onChange={(event) => onChange({ value: event.target.value })}
              placeholder="has_resp / qname domain:example.com"
              className="h-8 w-full font-mono text-xs"
              disabled={readOnly}
            />
          )}
        </div>
        {!readOnly && (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7 shrink-0 text-muted-foreground hover:text-destructive"
            onClick={onDelete}
            aria-label={t(WEBUI.sequence.deleteCondition)}
          >
            <Minus className="h-3.5 w-3.5" />
          </Button>
        )}
      </div>
    </div>
  );
}

function InvertCheckbox({
  checked,
  disabled,
  onCheckedChange,
}: {
  checked: boolean;
  disabled: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  const { t } = useI18n();
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className={cn(
            "flex h-8 w-6 shrink-0 items-center justify-center rounded-md border font-mono text-sm font-bold leading-none disabled:cursor-not-allowed disabled:opacity-50",
            checked
              ? "border-rose-400 bg-rose-500 text-white dark:border-rose-500 dark:bg-rose-600"
              : "border-input bg-background text-muted-foreground/25 hover:text-muted-foreground/60",
          )}
          aria-label={t(WEBUI.sequence.invertMatch)}
          disabled={disabled}
          onClick={() => onCheckedChange(!checked)}
        >
          !
        </button>
      </TooltipTrigger>
      <TooltipContent sideOffset={6}>
        {t(WEBUI.sequence.invertMatch)}
      </TooltipContent>
    </Tooltip>
  );
}

function SequenceReferencePreview({
  action,
  plugins,
  currentSequenceName,
  visitedSequences,
}: {
  action: SequenceAction;
  plugins: PluginInstance[];
  currentSequenceName?: string;
  visitedSequences: Set<string>;
}) {
  const { t } = useI18n();
  const target = getSequenceControlTarget(action);
  if (!target) return null;

  const isSelfReference = Boolean(
    currentSequenceName && target === currentSequenceName,
  );
  const isVisited = visitedSequences.has(target);
  const targetPlugin = plugins.find(
    (plugin) =>
      plugin.name === target &&
      plugin.type === "executor" &&
      plugin.pluginKind === "sequence",
  );

  // Preview cards adopt sky tinting so they read as visually continuous with
  // the sky-coloured branch edge that connects them to the rule node.
  if (isSelfReference || isVisited) {
    return (
      <div className="w-[360px] rounded-lg border border-dashed border-sky-300/70 bg-sky-50/50 px-3 py-2 text-xs text-sky-900 shadow-sm dark:border-sky-800/50 dark:bg-sky-950/30 dark:text-sky-200">
        {t(WEBUI.sequence.cycleRef, { control: action.control, target })}
      </div>
    );
  }

  if (!targetPlugin) {
    return (
      <div className="w-[360px] rounded-lg border border-dashed border-sky-300/70 bg-sky-50/50 px-3 py-2 text-xs text-sky-900 shadow-sm dark:border-sky-800/50 dark:bg-sky-950/30 dark:text-sky-200">
        {t(WEBUI.sequence.missingTarget, { control: action.control, target })}
      </div>
    );
  }

  const targetRules = parseSequenceRules(targetPlugin.config.args);

  return (
    <div className="w-max rounded-lg border border-sky-300/70 bg-sky-50/30 p-3 shadow-sm dark:border-sky-800/50 dark:bg-sky-950/15">
      <div className="mb-3 flex items-center gap-2 text-xs text-sky-700 dark:text-sky-300">
        <GitBranch className="h-3.5 w-3.5" />
        <span>
          {t(WEBUI.sequence.jumpTo, { control: action.control, target })}
        </span>
      </div>
      <div className="space-y-3">
        {targetRules.length > 0 ? (
          targetRules.map((rule, index) => (
            <SequenceRuleNode
              key={`${target}-${rule.id}`}
              rule={rule}
              index={index}
              total={targetRules.length}
              plugins={plugins}
              sequenceTags={[]}
              readOnly
              onChange={() => undefined}
              onMove={() => undefined}
              onDelete={() => undefined}
            />
          ))
        ) : (
          <div className="rounded-md border border-dashed px-3 py-4 text-center text-xs text-muted-foreground">
            {t(WEBUI.sequence.emptyTarget)}
          </div>
        )}
      </div>
    </div>
  );
}

function ActionEditor({
  action,
  plugins,
  sequenceTags,
  readOnly,
  onChange,
}: {
  action: SequenceAction;
  plugins: PluginInstance[];
  sequenceTags: string[];
  readOnly: boolean;
  onChange: (action: SequenceAction) => void;
}) {
  const { t } = useI18n();
  const controlArg = getControlArg(action);

  const updateMode = (mode: ActionMode) => {
    if (mode === "control") {
      onChange({
        mode,
        value: action.control,
        control: action.control,
      });
      return;
    }

    // Switching INTO quick_setup from anything else: seed with the first
    // quick-setup-capable executor kind (e.g. `drop_resp`). Switching INTO
    // text from non-text: reset to empty so the user starts fresh.
    let nextValue: string;
    if (mode === action.mode) {
      nextValue = action.value;
    } else if (mode === "quick_setup") {
      nextValue = defaultActionValue(mode);
    } else if (mode === "text") {
      nextValue = action.mode === "text" ? action.value : "";
    } else {
      nextValue = action.value || defaultActionValue(mode);
    }

    onChange({
      mode,
      value: nextValue,
      control: "accept",
    });
  };

  const updateControl = (control: ControlKind) => {
    onChange({
      mode: "control",
      control,
      value:
        control === "accept" || control === "return" ? control : `${control} `,
    });
  };

  return (
    <div className="w-full rounded-md border border-sky-200/80 bg-sky-50/40 p-2 dark:border-sky-800/40 dark:bg-sky-950/20">
      <div className="grid min-w-0 items-center gap-2 sm:grid-cols-[8rem_8rem_minmax(8rem,1fr)]">
        <InlineSelect
          value={action.mode}
          onChange={(mode) => updateMode(mode as ActionMode)}
          disabled={readOnly}
          className="w-full"
          options={[
            { value: "reference", label: t(WEBUI.sequence.modeReference) },
            { value: "quick_setup", label: t(WEBUI.sequence.modeQuickSetup) },
            { value: "control", label: t(WEBUI.sequence.modeControl) },
            { value: "text", label: t(WEBUI.sequence.modeText) },
          ]}
        />

        {action.mode === "reference" && (
          <div className="min-w-0 sm:col-span-2">
            <PluginReferencePicker
              plugins={plugins}
              value={stripReferencePrefix(action.value)}
              referenceTypes={["executor"]}
              disabled={readOnly}
              placeholder={t(WEBUI.sequence.selectExecutor)}
              className="h-8 min-h-8 py-0"
              allowCreate
              onChange={(tag) =>
                onChange({
                  mode: "reference",
                  value: `$${tag}`,
                  control: action.control,
                })
              }
            />
          </div>
        )}

        {action.mode === "quick_setup" && (
          <div className="min-w-0 sm:col-span-2">
            <QuickSetupRow
              type="executor"
              value={action.value}
              plugins={plugins}
              readOnly={readOnly}
              onChange={(next) =>
                onChange({
                  mode: "quick_setup",
                  value: next,
                  control: "accept",
                })
              }
            />
          </div>
        )}

        {action.mode === "text" && (
          <div className="min-w-0 sm:col-span-2">
            <Input
              value={action.value}
              onChange={(event) =>
                onChange({ ...action, value: event.target.value })
              }
              placeholder="forward 1.1.1.1 / ttl 300 / debug_print hit"
              className="h-8 w-full font-mono text-xs"
              disabled={readOnly}
            />
          </div>
        )}

        {action.mode === "control" && (
          <div className="contents">
            <InlineSelect
              value={action.control}
              onChange={(control) => updateControl(control as ControlKind)}
              disabled={readOnly}
              className="w-full"
              options={builtinControls.map((control) => ({
                value: control,
                label: controlLabels[control],
              }))}
            />
            {action.control === "accept" || action.control === "return" ? (
              <div className="hidden sm:block" />
            ) : action.control === "jump" || action.control === "goto" ? (
              <SequenceTargetInput
                value={controlArg}
                sequenceTags={sequenceTags}
                disabled={readOnly}
                onChange={(target) =>
                  onChange({
                    mode: "control",
                    control: action.control,
                    value: `${action.control} ${target}`.trim(),
                  })
                }
              />
            ) : (
              <Input
                value={controlArg}
                onChange={(event) =>
                  onChange({
                    mode: "control",
                    control: action.control,
                    value: `${action.control} ${event.target.value
                      .replace(/\s+/g, " ")
                      .trimStart()}`,
                  })
                }
                onBlur={(event) =>
                  onChange({
                    mode: "control",
                    control: action.control,
                    value: `${action.control} ${event.target.value}`
                      .replace(/\s+/g, " ")
                      .trim(),
                  })
                }
                placeholder={
                  action.control === "reject" ? "0 soa / 3" : "1,2,3"
                }
                className="h-8 max-w-[16rem] w-full font-mono text-xs"
                disabled={readOnly}
              />
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function SequenceTargetInput({
  value,
  sequenceTags,
  disabled,
  className,
  onChange,
}: {
  value: string;
  sequenceTags: string[];
  disabled: boolean;
  className?: string;
  onChange: (value: string) => void;
}) {
  const { t } = useI18n();
  if (sequenceTags.length === 0) {
    return (
      <div className="min-w-0 max-w-[16rem]">
        <Input
          value=""
          placeholder={t(WEBUI.sequence.noSequence)}
          className={`h-8 w-full min-w-0 max-w-full font-mono text-xs ${className ?? ""}`}
          disabled
        />
      </div>
    );
  }

  return (
    <div className="min-w-0 max-w-[16rem]">
      <InlineSelect
        value={value}
        onChange={onChange}
        placeholder={t(WEBUI.sequence.selectSequence)}
        className={`w-full min-w-0 font-mono text-xs ${className ?? ""}`}
        disabled={disabled}
        options={sequenceTags.map((tag) => ({
          value: tag,
          label: tag,
        }))}
      />
    </div>
  );
}

// ─── Standalone "create dependency plugin" entry ─────────────────────────────

function CreateDependencyPluginButton() {
  const { t } = useI18n();
  return (
    <CreatePluginDialog
      defaultType="matcher"
      supportedTypes={["executor", "matcher", "provider"]}
      title={t(WEBUI.sequence.createDepsTitle)}
      description={t(WEBUI.sequence.createDepsDesc)}
      trigger={
        <Button type="button" variant="outline" size="sm">
          <Plus className="h-4 w-4" />
          {t(WEBUI.sequence.createDepsBtn)}
        </Button>
      }
    />
  );
}

export function parseSequenceRules(value: unknown): SequenceRule[] {
  if (!Array.isArray(value)) return [];
  return value.map((entry, index) => {
    const record: Record<string, unknown> =
      entry && typeof entry === "object" && !Array.isArray(entry)
        ? (entry as Record<string, unknown>)
        : { exec: entry };
    const ruleId = createStableItemId("rule", index);
    return {
      id: ruleId,
      matches: parseMatches(record.matches, ruleId),
      action: parseAction(record.exec),
    };
  });
}

export function serializeSequenceRules(rules: SequenceRule[]) {
  return rules
    .map((rule) => {
      const entry: Record<string, unknown> = {};
      const matches = serializeMatches(rule.matches);
      const exec = serializeAction(rule.action);
      if (matches !== undefined) entry.matches = matches;
      if (exec || rule.action.mode === "text") entry.exec = exec;
      return entry;
    })
    .filter((entry) => Object.keys(entry).length > 0);
}

function parseMatches(value: unknown, ruleId: string): SequenceCondition[] {
  const entries = Array.isArray(value)
    ? value
    : typeof value === "string" && value.trim()
      ? [value]
      : [];

  return entries
    .map((entry) => (typeof entry === "string" ? entry.trim() : ""))
    .filter(Boolean)
    .map((entry, index) =>
      parseCondition(entry, createStableItemId(`${ruleId}_condition`, index)),
    );
}

function parseCondition(value: string, conditionId: string): SequenceCondition {
  const inverted = value.startsWith("!");
  const withoutInvert = inverted ? value.slice(1) : value;
  if (withoutInvert.startsWith("$")) {
    return {
      id: conditionId,
      mode: "reference",
      value: withoutInvert,
      invert: inverted,
    };
  }
  // Detect "qname $foo" / "client_ip 192.168.0.0/16" etc — quick_setup syntax.
  if (isQuickSetupValue(withoutInvert, "matcher")) {
    return {
      id: conditionId,
      mode: "quick_setup",
      value: withoutInvert,
      invert: inverted,
    };
  }
  return {
    id: conditionId,
    mode: "text",
    value: withoutInvert,
    invert: inverted,
  };
}

function parseAction(value: unknown): SequenceAction {
  const rawText = typeof value === "string" ? value : "";
  const text = rawText.trim();
  const control = inferControlKind(text);
  if (text.startsWith("$")) {
    return { mode: "reference", value: text, control: "accept" };
  }
  if (control) {
    return { mode: "control", value: rawText.trimStart(), control };
  }
  // quick_setup detection — recognise inline executor forms like
  // `query_summary main pipeline` or `drop_resp` before falling back to text.
  if (text && isQuickSetupValue(text, "executor")) {
    return { mode: "quick_setup", value: text, control: "accept" };
  }
  if (text) {
    return {
      mode: "text",
      value: text,
      control: "accept",
    };
  }
  return { mode: "text", value: "", control: "accept" };
}

function serializeMatches(matches: SequenceCondition[]) {
  const serialized = matches
    .map((condition) => {
      const value = condition.value.trim();
      if (!value) return "";
      if (condition.mode === "reference") {
        const tag = stripReferencePrefix(value);
        return `${condition.invert ? "!" : ""}$${tag}`;
      }
      // For `quick_setup` and `text` modes the stored value is already the
      // raw YAML form (e.g. "qname $domain_set", "has_resp 1"). Just prepend
      // the invert mark if needed.
      return `${condition.invert ? "!" : ""}${value}`;
    })
    .filter(Boolean);

  if (serialized.length === 0) return undefined;
  if (serialized.length === 1) return serialized[0];
  return serialized;
}

function serializeAction(action: SequenceAction) {
  if (action.mode === "reference") {
    const tag = stripReferencePrefix(action.value);
    return tag ? `$${tag}` : "";
  }
  if (action.mode === "control") {
    const arg = getControlArg(action);
    if (action.control === "accept" || action.control === "return") {
      return action.control;
    }
    const normalizedArg = arg.replace(/\s+/g, " ").trimStart();
    return normalizedArg.trim()
      ? `${action.control} ${normalizedArg}`
      : action.control;
  }
  // `quick_setup` and `text` modes both store the raw form already.
  return action.value.trim();
}

// Fingerprint a rule by its serialised body so persisted drag positions
// follow the rule even when its index in the array shifts. Two rules with
// identical YAML produce the same fingerprint; `buildSequenceFlow` then
// disambiguates duplicates with a `#N` occurrence suffix.
function rulePositionKey(rule: SequenceRule): string {
  const matches = serializeMatches(rule.matches);
  const exec = serializeAction(rule.action);
  // `matches` is `string | string[] | undefined` — normalise to a single
  // canonical string so equal-content rules hash identically regardless of
  // whether they were parsed from scalar or sequence YAML form.
  const matchesText = Array.isArray(matches)
    ? matches.join("\n")
    : (matches ?? "");
  return cyrb53(`${matchesText}>>>${exec}`);
}

// cyrb53 — fast, non-cryptographic 53-bit string hash. Good enough for
// localStorage keys (collisions are vanishingly rare and only cause two
// distinct rules to share a saved position, which the user can fix by
// re-dragging).
function cyrb53(str: string, seed = 0): string {
  let h1 = 0xdeadbeef ^ seed;
  let h2 = 0x41c6ce57 ^ seed;
  for (let i = 0; i < str.length; i++) {
    const ch = str.charCodeAt(i);
    h1 = Math.imul(h1 ^ ch, 2654435761);
    h2 = Math.imul(h2 ^ ch, 1597334677);
  }
  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507);
  h1 ^= Math.imul(h2 ^ (h2 >>> 13), 3266489909);
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507);
  h2 ^= Math.imul(h1 ^ (h1 >>> 13), 3266489909);
  return (4294967296 * (2097151 & h2) + (h1 >>> 0)).toString(36);
}

function createEmptyRule(): SequenceRule {
  return {
    id: createItemId(),
    matches: [],
    action: { mode: "control", value: "accept", control: "accept" },
  };
}

function createEmptyCondition(): SequenceCondition {
  return {
    id: createItemId(),
    mode: "quick_setup",
    value: firstQuickSetupKind("matcher") || "has_resp",
    invert: false,
  };
}

function defaultConditionValue(mode: ConditionMode) {
  if (mode === "reference") return "$has_resp";
  if (mode === "quick_setup") {
    const kind = firstQuickSetupKind("matcher") || "has_resp";
    return kind;
  }
  return "";
}

function defaultActionValue(mode: ActionMode) {
  if (mode === "reference") return "$forward_main";
  if (mode === "quick_setup") {
    const kind = firstQuickSetupKind("executor") || "drop_resp";
    return kind;
  }
  if (mode === "text") return "accept";
  return "accept";
}

function inferControlKind(value: string): ControlKind | null {
  const head = value.trim().split(/\s+/)[0];
  return builtinControls.includes(head as ControlKind)
    ? (head as ControlKind)
    : null;
}

function getControlArg(action: SequenceAction) {
  const expectedHead = `${action.control} `;
  if (action.value.startsWith(expectedHead))
    return action.value.slice(expectedHead.length);
  if (action.value === action.control) return "";
  return action.value.trim().split(/\s+/).slice(1).join(" ");
}

function getSequenceControlTarget(action: SequenceAction) {
  if (
    action.mode !== "control" ||
    (action.control !== "jump" && action.control !== "goto")
  ) {
    return "";
  }

  return getControlArg(action).trim();
}

function summarizeRule(rule: SequenceRule, t: (key: string) => string) {
  const matches =
    rule.matches.length === 0
      ? "always"
      : rule.matches
          .map((condition) => serializeMatches([condition]))
          .join(" && ");
  const action =
    serializeAction(rule.action) || t(WEBUI.sequence.unconfiguredAction);
  return `${matches} -> ${action}`;
}
