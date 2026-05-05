import { useTranslation } from "react-i18next";
import type { QAHistory } from "@/lib/ipc";

type Props = {
  items: QAHistory[];
  onPick: (item: QAHistory) => void;
};

function relTime(epochSec: number, lang: string): string {
  const now = Date.now() / 1000;
  const diff = Math.max(0, now - epochSec);
  const zh = lang.startsWith("zh");
  if (diff < 60) return zh ? "刚刚" : "just now";
  if (diff < 3600) {
    const m = Math.floor(diff / 60);
    return zh ? `${m} 分钟前` : `${m}m ago`;
  }
  if (diff < 86400) {
    const h = Math.floor(diff / 3600);
    return zh ? `${h} 小时前` : `${h}h ago`;
  }
  const d = Math.floor(diff / 86400);
  return zh ? `${d} 天前` : `${d}d ago`;
}

export default function HistoryList({ items, onPick }: Props) {
  const { t, i18n } = useTranslation();
  return (
    <aside className="w-[280px] shrink-0 border-l border-ink/10 bg-cream/40 p-4 overflow-y-auto">
      <h3 className="text-sm font-semibold text-ink/80 mb-3">
        {t("ask.history.title")}
      </h3>
      {items.length === 0 ? (
        <p className="text-xs text-ink/50">—</p>
      ) : (
        <ul className="space-y-2">
          {items.map((it) => (
            <li key={it.id}>
              <button
                type="button"
                onClick={() => onPick(it)}
                className="w-full text-left rounded-md border border-ink/10 bg-paper p-2 hover:border-accent/40 transition-colors"
              >
                <p className="text-sm text-ink line-clamp-2">{it.question}</p>
                <p className="mt-1 text-[11px] text-ink/50">
                  {relTime(it.created_at, i18n.language)}
                </p>
              </button>
            </li>
          ))}
        </ul>
      )}
    </aside>
  );
}
