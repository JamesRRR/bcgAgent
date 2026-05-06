import { useEffect, useRef, useState } from "react";
import { useSortable } from "@dnd-kit/sortable";
import { useTranslation } from "react-i18next";
import {
  AlertCircle,
  CheckCircle2,
  GripVertical,
  RefreshCw,
  X,
} from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { inTauri } from "@/lib/transport";

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

function useElapsedSeconds(active: boolean): number {
  const startRef = useRef<number | null>(null);
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    if (!active) {
      startRef.current = null;
      setElapsed(0);
      return;
    }
    startRef.current = Date.now();
    const id = window.setInterval(() => {
      if (startRef.current !== null) {
        setElapsed(Math.floor((Date.now() - startRef.current) / 1000));
      }
    }, 1000);
    return () => window.clearInterval(id);
  }, [active]);
  return elapsed;
}

function RunningBadge({ elapsed }: { elapsed: number }) {
  return (
    <span className="inline-flex items-center gap-1.5 text-xs text-accent">
      <RefreshCw className="w-3.5 h-3.5 animate-spin" />
      {elapsed > 0 && <span className="tabular-nums">{elapsed}s</span>}
    </span>
  );
}

function StatusBadge({
  status,
  elapsed,
  onRetry,
}: {
  status: PageStatus;
  elapsed: number;
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
      return <RunningBadge elapsed={elapsed} />;
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
  const { t } = useTranslation();
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: item.id, disabled });
  const elapsed = useElapsedSeconds(item.status.kind === "running");
  const showSlowHint = item.status.kind === "running" && elapsed >= 15;

  const style: React.CSSProperties = {
    transform: transform
      ? `translate3d(${transform.x}px, ${transform.y}px, 0)`
      : undefined,
    transition,
    opacity: isDragging ? 0.6 : 1,
  };

  // `convertFileSrc` only works inside the Tauri shell (it reads from
  // `window.__TAURI_INTERNALS__`). In browser/E2E mode we don't have a way to
  // read arbitrary local paths anyway — leave the thumb empty.
  const thumbSrc = inTauri ? convertFileSrc(item.path) : "";

  return (
    <div
      ref={setNodeRef}
      style={style}
      data-testid="page-card"
      data-status={item.status.kind}
      className="flex flex-col bg-paper border border-ink/10 rounded-md pl-1 pr-3 py-2 shadow-sm"
    >
      <div className="flex items-center gap-3">
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

      <StatusBadge status={item.status} elapsed={elapsed} onRetry={onRetry} />

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
      {showSlowHint && (
        <div className="ml-[4.25rem] mt-1 text-xs text-amber-700/80">
          {elapsed >= 60
            ? t("import.longWait")
            : t("import.slowNetwork")}
        </div>
      )}
    </div>
  );
}
