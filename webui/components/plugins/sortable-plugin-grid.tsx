"use client";

import { useState } from "react";
import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  MouseSensor,
  TouchSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  rectSortingStrategy,
  sortableKeyboardCoordinates,
  useSortable,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GripHorizontal } from "lucide-react";
import type { PluginInstance } from "@/lib/types";
import { PluginCard } from "@/components/plugins/plugin-card";
import { cn } from "@/lib/utils";

const DEFAULT_GRID_CLASS =
  "grid items-stretch gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4";

interface SortablePluginGridProps {
  plugins: PluginInstance[];
  /** Receives the new full id order of the visible cards after a drag. */
  onReorder: (orderedIds: string[]) => void;
  /** When true, cards render in a plain (non-draggable) grid. */
  disabled?: boolean;
  className?: string;
}

// A drag-to-arrange grid of plugin cards shared by the dashboard and the
// plugin center. Whole cards are draggable; a click (no movement) still opens
// the detail sheet thanks to the sensor activation thresholds, and a press on
// an inner button (pin/delete) is handled by that button. Keyboard users can
// tab to a card and use space + arrows to reorder.
export function SortablePluginGrid({
  plugins,
  onReorder,
  disabled = false,
  className,
}: SortablePluginGridProps) {
  const [activeId, setActiveId] = useState<string | null>(null);

  const sensors = useSensors(
    // Mouse: start dragging only after an 8px move so a plain click still
    // reaches the card's onClick (open detail).
    useSensor(MouseSensor, { activationConstraint: { distance: 8 } }),
    // Touch: press-and-hold to drag, so a tap still clicks and a swipe still
    // scrolls the page without grabbing a card.
    useSensor(TouchSensor, {
      activationConstraint: { delay: 200, tolerance: 8 },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  const gridClass = className ?? DEFAULT_GRID_CLASS;

  if (disabled) {
    return (
      <div className={gridClass}>
        {plugins.map((plugin) => (
          <PluginCard key={plugin.id} plugin={plugin} />
        ))}
      </div>
    );
  }

  const handleDragStart = (event: DragStartEvent) => {
    setActiveId(String(event.active.id));
  };

  const handleDragEnd = (event: DragEndEvent) => {
    setActiveId(null);
    const { active, over } = event;
    if (!over || active.id === over.id) return;
    const oldIndex = plugins.findIndex((p) => p.id === active.id);
    const newIndex = plugins.findIndex((p) => p.id === over.id);
    if (oldIndex < 0 || newIndex < 0) return;
    onReorder(arrayMove(plugins, oldIndex, newIndex).map((p) => p.id));
  };

  const activePlugin = activeId
    ? plugins.find((p) => p.id === activeId)
    : undefined;

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
      onDragCancel={() => setActiveId(null)}
    >
      <SortableContext
        items={plugins.map((p) => p.id)}
        strategy={rectSortingStrategy}
      >
        <div className={gridClass}>
          {plugins.map((plugin) => (
            <SortablePluginCard
              key={plugin.id}
              plugin={plugin}
              dragging={plugin.id === activeId}
            />
          ))}
        </div>
      </SortableContext>
      <DragOverlay>
        {activePlugin ? (
          <div className="rotate-1 cursor-grabbing opacity-95 shadow-xl">
            <PluginCard plugin={activePlugin} />
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}

function SortablePluginCard({
  plugin,
  dragging,
}: {
  plugin: PluginInstance;
  dragging: boolean;
}) {
  const { attributes, listeners, setNodeRef, transform, transition } =
    useSortable({ id: plugin.id });

  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className={cn(
        "group/sortable relative touch-manipulation",
        dragging && "opacity-40",
      )}
      {...attributes}
      {...listeners}
    >
      {/* Decorative grab affordance; the whole card is the drag target. */}
      <div className="pointer-events-none absolute left-1/2 top-0 z-10 flex -translate-x-1/2 items-center justify-center text-muted-foreground/50 opacity-0 transition-opacity group-hover/sortable:opacity-100">
        <GripHorizontal className="h-3.5 w-3.5" />
      </div>
      <PluginCard plugin={plugin} />
    </div>
  );
}
