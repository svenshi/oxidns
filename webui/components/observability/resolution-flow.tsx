"use client";

import { useMemo } from "react";
import {
  Background,
  Controls,
  MarkerType,
  Position,
  ReactFlow,
  type Edge,
  type Node,
} from "@xyflow/react";
import type { DNSObservation } from "@/lib/oxidns-api";
import { useI18n } from "@/lib/i18n/provider";
import { WEBUI } from "@/lib/i18n";

export default function ResolutionFlow({ row }: { row: DNSObservation }) {
  const { t, formatNumber } = useI18n();
  const { nodes, edges } = useMemo(() => {
    const root = `dns-query-${row.id}`;
    const eventNodes: Node[] = [];
    const eventEdges: Edge[] = [];
    const counts = new Map<string, number>();
    const byId = new Map(row.events.map((event) => [event.id, event]));
    const candidates = new Map(
      row.events
        .filter((event) => event.kind === "ip_candidate")
        .map((event) => [event.target, event.id]),
    );
    const graphEvents = row.events.filter(
      (event) =>
        event.kind !== "ip_probe" &&
        (event.kind !== "ip_candidate" ||
          candidates.get(event.target) === event.id),
    );
    const depths = new Map<number, number>();
    for (const event of graphEvents) {
      depths.set(event.id, (depths.get(event.parent_id ?? -1) ?? 0) + 1);
    }
    for (const event of graphEvents) {
      const parent =
        event.parent_id !== null && byId.has(event.parent_id)
          ? `dns-event-${row.id}-${event.parent_id}`
          : root;
      const depth = depths.get(event.id) ?? 1;
      const slot = counts.get(String(depth)) ?? 0;
      counts.set(String(depth), slot + 1);
      const title = event.target || event.plugin || event.kind;
      const latency =
        event.elapsed_ms === null
          ? event.status === "running"
            ? t(WEBUI.observability.untimed)
            : "—"
          : `${formatNumber(event.elapsed_ms, { maximumFractionDigits: 2 })} ms`;
      const details = [
        event.kind,
        event.method,
        event.status,
        event.selected ? t(WEBUI.observability.winner) : "",
        event.reason ?? "",
        ...row.events
          .filter(
            (probe) =>
              event.kind === "ip_candidate" &&
              probe.kind === "ip_probe" &&
              probe.target === event.target,
          )
          .map(
            (probe) =>
              `${probe.method}: ${probe.latency_ms === null ? probe.status : `${probe.latency_ms} ms`}`,
          ),
      ]
        .filter(Boolean)
        .join(" · ");
      eventNodes.push({
        sourcePosition: Position.Right,
        targetPosition: Position.Left,
        id: `dns-event-${row.id}-${event.id}`,
        position: { x: depth * 270, y: 20 + slot * 112 },
        data: {
          label: (
            <div className="w-[220px] overflow-hidden">
              <div
                className="truncate font-mono text-xs font-semibold"
                title={title}
              >
                {title || t(WEBUI.observability.event)}
              </div>
              <div
                className="mt-1 truncate text-[10px] opacity-70"
                title={details}
              >
                {details}
              </div>
              <div className="mt-1.5 flex items-center justify-between font-mono text-[10px]">
                <span>
                  {event.kind === "ip_candidate"
                    ? t(WEBUI.observability.selection)
                    : latency}
                </span>
                {event.latency_ms !== null && (
                  <span>{event.latency_ms} ms</span>
                )}
              </div>
            </div>
          ),
        },
        style: {
          width: 242,
          padding: "10px 12px",
          borderRadius: 9,
          borderColor: event.selected ? "var(--primary)" : "var(--border)",
          background:
            event.status === "not_started" ? "var(--muted)" : "var(--card)",
          opacity:
            event.status === "not_started" || event.status === "cancelled"
              ? 0.72
              : 1,
          color: "var(--foreground)",
          boxShadow: event.selected ? "0 0 0 1px var(--primary)" : undefined,
        },
      });
      eventEdges.push({
        id: `dns-edge-${row.id}-${event.id}`,
        source: parent,
        target: `dns-event-${row.id}-${event.id}`,
        animated: event.status === "running",
        label: event.selected ? t(WEBUI.observability.winner) : undefined,
        labelStyle: { fontSize: 10, fill: "var(--muted-foreground)" },
        style: {
          stroke: event.selected ? "var(--primary)" : "var(--border)",
          strokeWidth: event.selected ? 2 : 1.2,
        },
        markerEnd: {
          type: MarkerType.ArrowClosed,
          color: event.selected ? "var(--primary)" : "var(--muted-foreground)",
        },
      });
    }
    const query: Node = {
      sourcePosition: Position.Right,
      targetPosition: Position.Left,
      id: root,
      position: {
        x: 0,
        y: Math.max(90, Math.ceil(counts.get("1") ?? 1) * 42),
      },
      data: {
        label: (
          <div className="w-[208px]">
            <div className="font-mono text-xs font-semibold">{row.qname}</div>
            <div className="mt-1 text-[10px] opacity-70">
              {row.qtype} · {row.client_ip}
            </div>
            <div className="mt-1 font-mono text-[10px]">
              {row.rcode ?? row.status} ·{" "}
              {formatNumber(row.elapsed_ms, { maximumFractionDigits: 2 })} ms
            </div>
          </div>
        ),
      },
      style: {
        width: 232,
        padding: 12,
        borderRadius: 9,
        borderColor: "var(--primary)",
        background: "var(--card)",
        color: "var(--foreground)",
      },
    };
    const client: Node = {
      sourcePosition: Position.Right,
      targetPosition: Position.Left,
      id: `dns-client-${row.id}`,
      position: { x: -270, y: query.position.y },
      data: {
        label: (
          <div className="w-[205px]">
            <div className="text-xs font-semibold">
              {t(WEBUI.observability.client)}
            </div>
            <div className="mt-1 font-mono text-[10px]">{row.client_ip}</div>
          </div>
        ),
      },
      style: {
        width: 230,
        padding: 12,
        borderRadius: 9,
        borderColor: "var(--border)",
        background: "var(--card)",
        color: "var(--foreground)",
      },
    };
    const ingress: Edge = {
      id: `dns-ingress-${row.id}`,
      source: client.id,
      target: query.id,
      label: "DNS",
      labelStyle: { fontSize: 10, fill: "var(--muted-foreground)" },
      style: { stroke: "var(--primary)", strokeWidth: 1.5 },
      markerEnd: { type: MarkerType.ArrowClosed, color: "var(--primary)" },
    };
    const answers: Node[] = row.answers.slice(0, 6).map((answer, index) => ({
      sourcePosition: Position.Right,
      targetPosition: Position.Left,
      id: `dns-answer-${row.id}-${index}`,
      position: {
        x: (Math.max(1, ...depths.values()) + 1) * 270,
        y: 20 + index * 90,
      },
      data: {
        label: (
          <div className="w-[188px]">
            <div className="text-[10px] text-muted-foreground">
              {t(WEBUI.observability.answers)}
            </div>
            <div className="mt-1 truncate font-mono text-xs">{answer}</div>
          </div>
        ),
      },
      style: {
        width: 218,
        padding: 10,
        borderRadius: 9,
        borderColor: "var(--border)",
        background: "var(--card)",
        color: "var(--foreground)",
      },
    }));
    const answerEdges: Edge[] = answers.map((answer) => ({
      id: `dns-answer-edge-${answer.id}`,
      source: query.id,
      target: answer.id,
      style: { stroke: "var(--primary)", strokeWidth: 1.2 },
      markerEnd: { type: MarkerType.ArrowClosed, color: "var(--primary)" },
    }));
    return {
      nodes: [client, query, ...eventNodes, ...answers],
      edges: [ingress, ...eventEdges, ...answerEdges],
    };
  }, [formatNumber, row, t]);

  return (
    <div className="h-[540px] w-full bg-[radial-gradient(var(--border)_0.7px,transparent_0.7px)] [background-size:18px_18px]">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        fitView
        fitViewOptions={{ padding: 0.15 }}
        minZoom={0.25}
        maxZoom={1.5}
        nodesDraggable
        nodesConnectable={false}
        proOptions={{ hideAttribution: true }}
      >
        <Background color="var(--border)" gap={18} size={0.7} />
        <Controls showInteractive={false} />
      </ReactFlow>
    </div>
  );
}
