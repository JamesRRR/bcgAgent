import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { games as gamesIpc, type Game } from "@/lib/ipc";
import { useToaster } from "@/components/Toaster";

type Props = {
  onPicked: (gameId: string) => void;
};

export default function GamePicker({ onPicked }: Props) {
  const { t } = useTranslation();
  const toaster = useToaster();
  const [games, setGames] = useState<Game[] | null>(null);
  const [selectedId, setSelectedId] = useState<string>("");
  const [creating, setCreating] = useState(false);
  const [nameZh, setNameZh] = useState("");
  const [nameEn, setNameEn] = useState("");
  const [publisher, setPublisher] = useState("");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    let cancelled = false;
    gamesIpc
      .list()
      .then((g) => {
        if (cancelled) return;
        setGames(g);
        if (g.length > 0) setSelectedId(g[0].id);
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

  if (games === null) {
    return <p className="text-ink/60">{t("common.loading")}</p>;
  }

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = nameZh.trim();
    if (!trimmed || submitting) return;
    setSubmitting(true);
    try {
      const newId = await gamesIpc.create(
        trimmed,
        nameEn.trim() || undefined,
        publisher.trim() || undefined,
      );
      onPicked(newId);
    } catch (err) {
      toaster.push(String(err), "error");
      setSubmitting(false);
    }
  };

  return (
    <div className="max-w-md space-y-6">
      {games.length > 0 && !creating && (
        <div className="space-y-3">
          <label className="block text-sm text-ink/70">
            {t("nav.library")}
          </label>
          <select
            value={selectedId}
            onChange={(e) => setSelectedId(e.target.value)}
            className="w-full h-10 rounded-md border border-ink/20 bg-cream px-3 text-ink focus:outline-none focus:ring-2 focus:ring-accent"
          >
            {games.map((g) => (
              <option key={g.id} value={g.id}>
                {g.name_zh}
              </option>
            ))}
          </select>
          <div className="flex gap-2">
            <Button
              onClick={() => selectedId && onPicked(selectedId)}
              disabled={!selectedId}
            >
              {t("common.confirm")}
            </Button>
            <Button variant="outline" onClick={() => setCreating(true)}>
              <Plus className="w-4 h-4 mr-1" />
              {t("library.addGame")}
            </Button>
          </div>
        </div>
      )}

      {(games.length === 0 || creating) && (
        <form onSubmit={handleCreate} className="space-y-3">
          <h2 className="text-lg font-semibold text-ink">
            {t("library.addGameDialog.title")}
          </h2>
          <label className="block">
            <span className="block text-sm text-ink/70 mb-1">
              {t("library.addGameDialog.nameZhLabel")}
            </span>
            <input
              autoFocus
              type="text"
              value={nameZh}
              onChange={(e) => setNameZh(e.target.value)}
              placeholder={t("library.addGameDialog.nameZhPlaceholder")}
              className="w-full h-10 rounded-md border border-ink/20 bg-cream px-3 text-ink focus:outline-none focus:ring-2 focus:ring-accent"
              required
            />
          </label>
          <label className="block">
            <span className="block text-sm text-ink/70 mb-1">
              {t("library.addGameDialog.nameEnLabel")}
            </span>
            <input
              type="text"
              value={nameEn}
              onChange={(e) => setNameEn(e.target.value)}
              className="w-full h-10 rounded-md border border-ink/20 bg-cream px-3 text-ink focus:outline-none focus:ring-2 focus:ring-accent"
            />
          </label>
          <label className="block">
            <span className="block text-sm text-ink/70 mb-1">
              {t("library.addGameDialog.publisherLabel")}
            </span>
            <input
              type="text"
              value={publisher}
              onChange={(e) => setPublisher(e.target.value)}
              className="w-full h-10 rounded-md border border-ink/20 bg-cream px-3 text-ink focus:outline-none focus:ring-2 focus:ring-accent"
            />
          </label>
          <div className="flex gap-2 pt-1">
            <Button type="submit" disabled={!nameZh.trim() || submitting}>
              {t("library.addGameDialog.confirm")}
            </Button>
            {games.length > 0 && (
              <Button
                type="button"
                variant="outline"
                onClick={() => setCreating(false)}
              >
                {t("library.addGameDialog.cancel")}
              </Button>
            )}
          </div>
        </form>
      )}
    </div>
  );
}
