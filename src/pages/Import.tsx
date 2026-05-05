import { useEffect } from "react";
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
import { Button } from "@/components/ui/button";
import { useApp } from "@/state";
import { useToaster } from "@/components/Toaster";
import { useIngestCtx, makeItem } from "@/components/IngestProvider";
import Dropzone from "@/components/import/Dropzone";
import GamePicker from "@/components/import/GamePicker";
import PageCard, { type PageItem } from "@/components/import/PageCard";

export default function Import() {
  const { t } = useTranslation();
  const { selectedGameId } = useApp();
  const toaster = useToaster();
  const { state, setItems, setGameId, start, retry } = useIngestCtx();
  const { gameId, items, running } = state;

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
  );

  // Sync to the gameId chosen elsewhere (e.g. "create new game" flow). Skip
  // while a run is active — clobbering mid-import would orphan progress.
  useEffect(() => {
    if (running) return;
    if (selectedGameId && selectedGameId !== gameId) {
      setGameId(selectedGameId);
      // New game → fresh basket of pages.
      setItems(() => []);
    }
  }, [selectedGameId, gameId, running, setGameId, setItems]);

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

  const handleStart = async () => {
    if (!gameId || items.length === 0 || running) return;
    try {
      await start(gameId, items.map((i) => i.path));
    } catch (e) {
      toaster.push(String(e), "error");
    }
  };

  const handleRetry = async (item: PageItem) => {
    if (!gameId) return;
    try {
      await retry(gameId, item);
    } catch (e) {
      toaster.push(String(e), "error");
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
  const canStart = items.length > 0 && !running;

  return (
    <section className="px-10 py-12">
      <h1 className="text-3xl font-semibold text-ink mb-6">
        {t("import.title")}
      </h1>

      <div className="max-w-2xl space-y-6">
        <Dropzone disabled={running} onPicked={handlePicked} />

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
                      disabled={running}
                      onRemove={
                        running ? undefined : () => handleRemove(item.id)
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
