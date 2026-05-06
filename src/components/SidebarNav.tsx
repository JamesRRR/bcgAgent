import { useTranslation } from "react-i18next";
import { useApp, type Page } from "@/state";
import { useIngestCtx } from "@/components/IngestProvider";
import { cn } from "@/lib/utils";

const NAV_ITEMS: { key: Page; label: string }[] = [
  { key: "library", label: "nav.library" },
  { key: "import", label: "nav.import" },
  { key: "walkthrough", label: "nav.walkthrough" },
  { key: "ask", label: "nav.ask" },
  { key: "settings", label: "nav.settings" },
];

export default function SidebarNav() {
  const { t } = useTranslation();
  const { page, setPage } = useApp();
  const { state: ingestState } = useIngestCtx();

  // Compute progress only while a run is active.
  const ingestProgress = (() => {
    if (!ingestState.running) return null;
    const total = ingestState.items.length;
    if (total === 0) return null;
    const done = ingestState.items.filter(
      (it) => it.status.kind === "done" || it.status.kind === "failed",
    ).length;
    return { done, total };
  })();

  return (
    <aside className="w-[220px] shrink-0 bg-cream border-r border-ink/10 flex flex-col">
      <div className="px-6 py-6 flex items-center gap-2">
        <span className="text-2xl">🎲</span>
        <span className="font-semibold tracking-wide text-ink">攀达桌游</span>
      </div>
      <nav className="px-3 py-2 flex flex-col gap-1">
        {NAV_ITEMS.map((item) => {
          const active = page === item.key;
          const showProgress = item.key === "import" && ingestProgress;
          return (
            <button
              key={item.key}
              type="button"
              onClick={() => setPage(item.key)}
              className={cn(
                "text-left rounded-md px-3 py-2 text-sm transition-colors flex items-center justify-between gap-2",
                active
                  ? "bg-accent text-cream"
                  : "text-ink/80 hover:bg-paper",
              )}
            >
              <span>{t(item.label)}</span>
              {showProgress && (
                <span
                  data-testid="ingest-progress-badge"
                  className={cn(
                    "text-xs px-1.5 py-0.5 rounded-full font-medium",
                    active
                      ? "bg-cream/30 text-cream"
                      : "bg-accent/15 text-accent",
                  )}
                  title={t("import.progress.running")}
                >
                  {ingestProgress.done}/{ingestProgress.total}
                </span>
              )}
            </button>
          );
        })}
      </nav>
    </aside>
  );
}
