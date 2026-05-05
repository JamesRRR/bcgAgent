import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import EmptyShelf from "@/components/EmptyShelf";
import GameCard from "@/components/library/GameCard";
import NewGameDialog from "@/components/library/NewGameDialog";
import RenameGameDialog from "@/components/library/RenameGameDialog";
import { games as gamesIpc, type Game } from "@/lib/ipc";
import { useApp } from "@/state";
import { useToaster } from "@/components/Toaster";

export default function Library() {
  const { t } = useTranslation();
  const { setPage } = useApp();
  const toaster = useToaster();
  const [games, setGames] = useState<Game[] | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [renaming, setRenaming] = useState<Game | null>(null);

  useEffect(() => {
    let cancelled = false;
    gamesIpc
      .list()
      .then((g) => {
        if (!cancelled) setGames(g);
      })
      .catch((e) => {
        if (!cancelled) {
          toaster.push(String(e), "error");
          setGames([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [toaster]);

  const handleCreate = async (
    name_zh: string,
    name_en: string | undefined,
    publisher: string | undefined,
  ) => {
    try {
      const newId = await gamesIpc.create(name_zh, name_en, publisher);
      toaster.push(`${t("library.title")} ✓`, "success");
      setDialogOpen(false);
      setPage("import", newId);
    } catch (e) {
      toaster.push(String(e), "error");
    }
  };

  const handleRename = async (
    name_zh: string,
    name_en: string | undefined,
  ) => {
    if (!renaming) return;
    try {
      await gamesIpc.rename(renaming.id, name_zh, name_en);
      setGames((cur) =>
        cur
          ? cur.map((g) =>
              g.id === renaming.id
                ? { ...g, name_zh, name_en: name_en ?? null }
                : g,
            )
          : cur,
      );
      setRenaming(null);
      toaster.push(`${t("library.rename")} ✓`, "success");
    } catch (e) {
      toaster.push(String(e), "error");
    }
  };

  // Loading state
  if (games === null) {
    return (
      <section className="px-10 py-12">
        <h1 className="text-5xl font-handwritten text-ink mb-8">
          {t("library.title")}
        </h1>
        <p className="text-ink/60 text-center py-16">{t("common.loading")}</p>
      </section>
    );
  }

  // Empty state
  if (games.length === 0) {
    return (
      <section className="px-10 py-24 flex flex-col items-center">
        <EmptyShelf />
        <Button className="mt-2" onClick={() => setDialogOpen(true)}>
          <Plus className="w-4 h-4 mr-2" />
          {t("library.addGame")}
        </Button>
        <NewGameDialog
          open={dialogOpen}
          onClose={() => setDialogOpen(false)}
          onConfirm={handleCreate}
        />
      </section>
    );
  }

  // Populated state
  return (
    <section className="px-10 py-12">
      <div className="flex items-end justify-between mb-3">
        <h1 className="text-5xl font-handwritten text-ink">
          {t("library.title")}
        </h1>
        <Button onClick={() => setDialogOpen(true)}>
          <Plus className="w-4 h-4 mr-2" />
          {t("library.addGame")}
        </Button>
      </div>

      <div
        className="h-[3px] bg-shelf rounded-sm shadow-[0_2px_4px_rgba(0,0,0,0.15)] mb-8"
        aria-hidden="true"
      />

      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-6">
        {games.map((g) => (
          <GameCard
            key={g.id}
            game={g}
            onClick={() => setPage("handbook", g.id)}
            onRename={() => setRenaming(g)}
          />
        ))}
      </div>

      <NewGameDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        onConfirm={handleCreate}
      />
      <RenameGameDialog
        open={renaming !== null}
        initialNameZh={renaming?.name_zh ?? ""}
        initialNameEn={renaming?.name_en ?? null}
        onClose={() => setRenaming(null)}
        onConfirm={handleRename}
      />
    </section>
  );
}
