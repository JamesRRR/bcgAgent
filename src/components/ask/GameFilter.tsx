import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { games as gamesIpc, type Game } from "@/lib/ipc";
import { useToaster } from "@/components/Toaster";

type Props = {
  value: string | null;
  onChange: (gameId: string | null) => void;
};

export default function GameFilter({ value, onChange }: Props) {
  const { t } = useTranslation();
  const toaster = useToaster();
  const [games, setGames] = useState<Game[]>([]);
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    gamesIpc
      .list()
      .then((g) => {
        if (!cancelled) setGames(g);
      })
      .catch((e) => {
        if (!cancelled) toaster.push(String(e), "error");
      });
    return () => {
      cancelled = true;
    };
  }, [toaster]);

  useEffect(() => {
    if (!open) return;
    const onClick = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    window.addEventListener("mousedown", onClick);
    return () => window.removeEventListener("mousedown", onClick);
  }, [open]);

  const current = value ? games.find((g) => g.id === value) : null;
  const label = current ? current.name_zh : t("ask.gameFilterAll");

  return (
    <div className="relative" ref={wrapRef}>
      <Button
        variant="outline"
        size="sm"
        onClick={() => setOpen((v) => !v)}
        className="gap-1"
      >
        <span>{label}</span>
        <ChevronDown className="w-3 h-3" />
      </Button>
      {open && (
        <div className="absolute left-0 top-full mt-1 z-20 min-w-[180px] rounded-md border border-ink/15 bg-paper shadow-lg py-1">
          <button
            type="button"
            onClick={() => {
              onChange(null);
              setOpen(false);
            }}
            className="w-full flex items-center gap-2 px-3 py-1.5 text-sm text-ink hover:bg-cream"
          >
            <span className="w-3.5">
              {value === null && <Check className="w-3.5 h-3.5" />}
            </span>
            <span>{t("ask.gameFilterAll")}</span>
          </button>
          <div className="my-1 h-px bg-ink/10" />
          {games.map((g) => (
            <button
              key={g.id}
              type="button"
              onClick={() => {
                onChange(g.id);
                setOpen(false);
              }}
              className="w-full flex items-center gap-2 px-3 py-1.5 text-sm text-ink hover:bg-cream"
            >
              <span className="w-3.5">
                {value === g.id && <Check className="w-3.5 h-3.5" />}
              </span>
              <span className="truncate">{g.name_zh}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
