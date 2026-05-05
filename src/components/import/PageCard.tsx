import { useSortable } from "@dnd-kit/sortable";
import {
  AlertCircle,
  CheckCircle2,
  GripVertical,
  RefreshCw,
  X,
} from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";

export type PageStatus =
  | { kind: "pending" }
  | { kind: "running" }
  | { kind: "done"; chunkCount: number }
  | { kind: "failed"; error: string };

export type PageItem = {
  id: string; // stable id used by dnd-kit
  path: string;
  status: PageStatus;
};

type Props = {
  item: PageItem;
  index: number;
  disabled?: boolean;
  onRemove?: () => void;
  onRetry?: () => void;
};

function basename(p: string): string {
  const m = p.match(/[^/\\]+$/);
  return m ? m[0] : p;
}

function StatusBadge({
  status,
  onRetry,
}: {
  status: PageStatus;
  onRetry?: () => void;
}) {
  switch (status.kind) {
    case "pending":
      return (
        <span className="inline-flex items-center gap-1.5 text-xs text-ink/50">
          <span className="w-2 h-2 rounded-full bg-ink/30" />
        </span>
      );
    case "running":
      return (
        <span className="inline-flex items-center gap-1.5 text-xs text-accent">
          <RefreshCw className="w-3.5 h-3.5 animate-spin" />
        </span>
      );
    case "done":
      return (
        <span className="inline-flex items-center gap-1.5 text-xs text-accent">
          <CheckCircle2 className="w-4 h-4" />
          <span>{status.chunkCount}</span>
        </span>
      );
    case "failed":
      return (
        <span className="inline-flex items-center gap-1.5 text-xs text-red-500">
          <AlertCircle className="w-4 h-4" />
          {onRetry && (
            <button
              type="button"
              onClick={onRetry}
              className="underline hover:text-red-600"
            >
              重试
            </button>
          )}
        </span>
      );
  }
}

export default function PageCard({
  item,
  index,
  disabled,
  onRemove,
  onRetry,
}: Props) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: item.id, disabled });

  const style: React.CSSProperties = {
    transform: transform
      ? `translate3d(${transform.x}px, ${transform.y}px, 0)`
      : undefined,
    transition,
    opacity: isDragging ? 0.6 : 1,
  };

  const thumbSrc = convertFileSrc(item.path);

  return (
    <div
      ref={setNodeRef}
      style={style}
      className="flex items-center gap-3 bg-paper border border-ink/10 rounded-md pl-1 pr-3 py-2 shadow-sm"
    >
      <button
        type="button"
        {...attributes}
        {...listeners}
        disabled={disabled}
        aria-label="drag handle"
        className="p-1 text-ink/40 hover:text-ink/70 cursor-grab active:cursor-grabbing disabled:cursor-not-allowed touch-none"
      >
        <GripVertical className="w-5 h-5" />
      </button>

      <span className="text-xs text-ink/40 w-6 text-center font-mono">
        {index + 1}
      </span>

      <img
        src={thumbSrc}
        alt=""
        className="w-12 h-12 object-cover rounded-sm bg-cream border border-ink/10 flex-shrink-0"
        loading="lazy"
      />

      <span className="flex-1 truncate text-sm text-ink" title={item.path}>
        {basename(item.path)}
      </span>

      <StatusBadge status={item.status} onRetry={onRetry} />

      {onRemove && (
        <button
          type="button"
          onClick={onRemove}
          disabled={disabled}
          aria-label="remove"
          className="p-1 text-ink/40 hover:text-red-500 disabled:opacity-30 disabled:cursor-not-allowed"
        >
          <X className="w-4 h-4" />
        </button>
      )}
    </div>
  );
}
