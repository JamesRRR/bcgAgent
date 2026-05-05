import { useTranslation } from "react-i18next";
import { useApp, type Page } from "@/state";
import { cn } from "@/lib/utils";

const NAV_ITEMS: { key: Page; label: string }[] = [
  { key: "library", label: "nav.library" },
  { key: "import", label: "nav.import" },
  { key: "ask", label: "nav.ask" },
  { key: "settings", label: "nav.settings" },
];

export default function SidebarNav() {
  const { t } = useTranslation();
  const { page, setPage } = useApp();

  return (
    <aside className="w-[220px] shrink-0 bg-cream border-r border-ink/10 flex flex-col">
      <div className="px-6 py-6 flex items-center gap-2">
        <span className="text-2xl">🎲</span>
        <span className="font-semibold tracking-wide text-ink">bcgAgent</span>
      </div>
      <nav className="px-3 py-2 flex flex-col gap-1">
        {NAV_ITEMS.map((item) => {
          const active = page === item.key;
          return (
            <button
              key={item.key}
              type="button"
              onClick={() => setPage(item.key)}
              className={cn(
                "text-left rounded-md px-3 py-2 text-sm transition-colors",
                active
                  ? "bg-accent text-cream"
                  : "text-ink/80 hover:bg-paper",
              )}
            >
              {t(item.label)}
            </button>
          );
        })}
      </nav>
    </aside>
  );
}
