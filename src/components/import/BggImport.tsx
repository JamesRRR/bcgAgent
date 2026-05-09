import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { bgg, type BggMatch } from "@/lib/ipc";
import { useToaster } from "@/components/Toaster";

type Props = {
  /** When provided, BGG content is appended to this game. Otherwise a new
   *  game row is created with the BGG primary name. */
  existingGameId: string | null;
  onImported: (gameId: string, pageCount: number, chunkCount: number) => void;
};

export default function BggImport({ existingGameId, onImported }: Props) {
  const { t, i18n } = useTranslation();
  const toaster = useToaster();
  const isZh = i18n.language === "zh-CN";

  const [query, setQuery] = useState("");
  const [searching, setSearching] = useState(false);
  const [results, setResults] = useState<BggMatch[]>([]);
  const [importingId, setImportingId] = useState<number | null>(null);

  const handleSearch = async () => {
    const q = query.trim();
    if (!q || searching) return;
    setSearching(true);
    try {
      const r = await bgg.search(q);
      setResults(r);
      if (r.length === 0) {
        toaster.push(isZh ? "没有匹配结果" : "No matches found", "info");
      }
    } catch (e) {
      toaster.push(String(e), "error");
    } finally {
      setSearching(false);
    }
  };

  const handleImport = async (m: BggMatch) => {
    if (importingId !== null) return;
    setImportingId(m.id);
    try {
      const result = await bgg.importFromBgg(m.id, null, existingGameId);
      onImported(result.game_id, result.page_count, result.chunk_count);
      toaster.push(
        isZh
          ? `已从 BGG 导入 ${result.page_count} 页 / ${result.chunk_count} 段`
          : `Imported ${result.page_count} pages / ${result.chunk_count} chunks from BGG`,
        "success",
      );
    } catch (e) {
      toaster.push(String(e), "error");
    } finally {
      setImportingId(null);
    }
  };

  return (
    <div className="rounded-md border border-ink/10 bg-paper p-4 space-y-3">
      <h2 className="text-base font-medium text-ink">
        {t("import.bgg.title")}
      </h2>
      <p className="text-xs text-ink/55">{t("import.bgg.intro")}</p>
      <div className="flex gap-2">
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              handleSearch();
            }
          }}
          placeholder={t("import.bgg.placeholder") as string}
          className="flex-1 rounded-md border border-ink/15 bg-cream/40 px-3 py-2 text-sm outline-none focus:border-accent"
        />
        <Button onClick={handleSearch} disabled={searching || !query.trim()}>
          {searching ? (
            <Loader2 className="w-4 h-4 mr-2 animate-spin" />
          ) : (
            <Search className="w-4 h-4 mr-2" />
          )}
          {t("import.bgg.search")}
        </Button>
      </div>

      {results.length > 0 && (
        <ul className="divide-y divide-ink/10 border border-ink/10 rounded-md max-h-64 overflow-y-auto">
          {results.map((m) => (
            <li
              key={m.id}
              className="flex items-center justify-between gap-3 px-3 py-2"
            >
              <div className="min-w-0">
                <div className="text-sm text-ink truncate">{m.name}</div>
                <div className="text-xs text-ink/50">
                  BGG #{m.id}
                  {m.year ? ` · ${m.year}` : ""}
                </div>
              </div>
              <Button
                size="sm"
                variant="outline"
                onClick={() => handleImport(m)}
                disabled={importingId !== null}
              >
                {importingId === m.id ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  t("import.bgg.use")
                )}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
