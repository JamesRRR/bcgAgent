import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  DndContext,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { Button } from "@/components/ui/button";
import { useApp } from "@/state";
import { useToaster } from "@/components/Toaster";
import { ingest } from "@/lib/ipc";
import Dropzone from "@/components/import/Dropzone";
import GamePicker from "@/components/import/GamePicker";
import PageCard, {
  type PageItem,
  type PageStatus,
} from "@/components/import/PageCard";

let nextItemId = 0;
const makeItem = (path: string): PageItem => ({
  id: `pi-${++nextItemId}`,
  path,
  status: { kind: "pending" },
});

export default function Import() {
  const { t } = useTranslation();
  const { selectedGameId, setPage } = useApp();
  const toaster = useToaster();

  const [gameId, setGameId] = useState<string | null>(selectedGameId);
  const [items, setItems] = useState<PageItem[]>([]);
  const [importing, setImporting] = useState(false);

  // Stable refs so listener callbacks always see latest paths.
  const pathsAtRunRef = useRef<string[]>([]);
  const unlistenersRef = useRef<UnlistenFn[]>([]);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
  );

  // Keep gameId in sync if selectedGameId changes externally.
  useEffect(() => {
    if (selectedGameId && selectedGameId !== gameId) setGameId(selectedGameId);
  }, [selectedGameId, gameId]);

  // Cleanup listeners on unmount.
  useEffect(() => {
    return () => {
      for (const u of unlistenersRef.current) u();
      unlistenersRef.current = [];
    };
  }, []);

  const updateStatusByPageNumber = useCallback(
    (pageNumber: number, status: PageStatus) => {
      const path = pathsAtRunRef.current[pageNumber - 1];
      if (!path) return;
      setItems((cur) =>
        cur.map((it) => (it.path === path ? { ...it, status } : it)),
      );
    },
    [],
  );

  const handleDragEnd = (e: DragEndEvent) => {
    const { active, over } = e;
    if (!over || active.id === over.id) return;
    setItems((cur) => {
      const from = cur.findIndex((i) => i.id === active.id);
      const to = cur.findIndex((i) => i.id === over.id);
      if (from < 0 || to < 0) return cur;
      return arrayMove(cur, from, to);
    });
  };

  const handlePicked = (paths: string[]) => {
    setItems((cur) => {
      const existing = new Set(cur.map((i) => i.path));
      const fresh = paths.filter((p) => !existing.has(p)).map(makeItem);
      return [...cur, ...fresh];
    });
  };

  const handleRemove = (id: string) => {
    setItems((cur) => cur.filter((i) => i.id !== id));
  };

  const runImport = async (paths: string[]) => {
    if (!gameId || paths.length === 0) return;

    // CRITICAL: register listeners BEFORE invoking ingest.run.
    const unlisteners: UnlistenFn[] = [];
    unlisteners.push(
      await ingest.onPageStarted(({ page_number }) =>
        updateStatusByPageNumber(page_number, { kind: "running" }),
      ),
      await ingest.onPageDone(({ page_number, chunk_count }) =>
        updateStatusByPageNumber(page_number, {
          kind: "done",
          chunkCount: chunk_count,
        }),
      ),
      await ingest.onPageFailed(({ page_number, error }) =>
        updateStatusByPageNumber(page_number, { kind: "failed", error }),
      ),
      await ingest.onDone(({ succeeded, failed, game_id }) => {
        toaster.push(
          `${t("import.progress.done")} ${succeeded}/${succeeded + failed}`,
          failed > 0 ? "error" : "success",
        );
        setPage("handbook", game_id);
      }),
    );
    // Append rather than replace so a retry-during-run keeps prior listeners.
    unlistenersRef.current.push(...unlisteners);

    pathsAtRunRef.current = paths;
    try {
      await ingest.run(gameId, paths);
    } catch (e) {
      toaster.push(String(e), "error");
    }
  };

  const handleStart = async () => {
    if (!gameId || items.length === 0 || importing) return;
    setImporting(true);
    // Reset all to pending in case of re-run.
    setItems((cur) => cur.map((it) => ({ ...it, status: { kind: "pending" } })));
    const paths = items.map((i) => i.path);
    await runImport(paths);
  };

  const handleRetry = async (item: PageItem) => {
    if (!gameId) return;
    // Find this item's index in current ordering — backend will report
    // page_number=1 for a single-path run, so we map by re-aliasing.
    setItems((cur) =>
      cur.map((i) =>
        i.id === item.id ? { ...i, status: { kind: "running" } } : i,
      ),
    );
    // For a single-page retry, page_number from backend will be 1, mapping
    // to the only path in the request.
    pathsAtRunRef.current = [item.path];
    try {
      await ingest.run(gameId, [item.path]);
    } catch (e) {
      toaster.push(String(e), "error");
      setItems((cur) =>
        cur.map((i) =>
          i.id === item.id
            ? { ...i, status: { kind: "failed", error: String(e) } }
            : i,
        ),
      );
    }
  };

  // Step 1: pick or create a game.
  if (!gameId) {
    return (
      <section className="px-10 py-12">
        <h1 className="text-3xl font-semibold text-ink mb-6">
          {t("import.title")}
        </h1>
        <GamePicker onPicked={(id) => setGameId(id)} />
      </section>
    );
  }

  // Step 2/3: file selection, reorder, progress.
  const canStart = items.length > 0 && !importing;

  return (
    <section className="px-10 py-12">
      <h1 className="text-3xl font-semibold text-ink mb-6">
        {t("import.title")}
      </h1>

      <div className="max-w-2xl space-y-6">
        <Dropzone disabled={importing} onPicked={handlePicked} />

        {items.length > 0 && (
          <>
            <p className="text-sm text-ink/60">{t("import.reorderHint")}</p>
            <DndContext
              sensors={sensors}
              collisionDetection={closestCenter}
              onDragEnd={handleDragEnd}
            >
              <SortableContext
                items={items.map((i) => i.id)}
                strategy={verticalListSortingStrategy}
              >
                <div className="space-y-2">
                  {items.map((item, i) => (
                    <PageCard
                      key={item.id}
                      item={item}
                      index={i}
                      disabled={importing}
                      onRemove={
                        importing ? undefined : () => handleRemove(item.id)
                      }
                      onRetry={
                        item.status.kind === "failed"
                          ? () => handleRetry(item)
                          : undefined
                      }
                    />
                  ))}
                </div>
              </SortableContext>
            </DndContext>
          </>
        )}

        <div className="flex justify-end">
          <Button onClick={handleStart} disabled={!canStart}>
            {t("import.startButton")}
          </Button>
        </div>
      </div>
    </section>
  );
}
