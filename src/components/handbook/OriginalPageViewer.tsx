import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ZoomIn } from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { inTauri } from "@/lib/transport";
import type { Page } from "@/lib/ipc";

type Props = {
  page: Page | null;
};

export default function OriginalPageViewer({ page }: Props) {
  const { t } = useTranslation();
  const [zoomed, setZoomed] = useState(false);

  useEffect(() => {
    if (!zoomed) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setZoomed(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [zoomed]);

  // Reset zoom if the active page changes.
  useEffect(() => {
    setZoomed(false);
  }, [page?.id]);

  if (!page) {
    return (
      <aside className="w-[240px] shrink-0 border-l border-ink/10 bg-cream/40" />
    );
  }

  // `convertFileSrc` only works inside the Tauri shell; in browser/E2E we
  // can't read arbitrary local paths so we just leave the image src empty.
  const thumbSrc = inTauri
    ? convertFileSrc(page.thumb_path || page.image_path)
    : "";
  const fullSrc = inTauri ? convertFileSrc(page.image_path) : "";

  return (
    <>
      <aside className="w-[240px] shrink-0 border-l border-ink/10 bg-cream/40 overflow-y-auto">
        <div className="p-4 sticky top-0">
          <div className="text-xs text-ink/50 mb-2 flex items-center justify-between">
            <span>p. {page.page_number}</span>
            <span>{t("handbook.viewOriginal")}</span>
          </div>
          <button
            type="button"
            onClick={() => setZoomed(true)}
            className="group relative w-full rounded-md overflow-hidden border border-ink/10 bg-paper hover:border-accent/40 transition-colors"
            aria-label={t("handbook.viewOriginal")}
          >
            <img
              src={thumbSrc}
              alt={`page ${page.page_number}`}
              className="w-full h-auto block"
              draggable={false}
            />
            <span className="absolute inset-0 flex items-center justify-center bg-ink/0 group-hover:bg-ink/30 transition-colors">
              <ZoomIn className="w-6 h-6 text-cream opacity-0 group-hover:opacity-100 transition-opacity" />
            </span>
          </button>
        </div>
      </aside>

      {zoomed && (
        <div
          className="fixed inset-0 z-50 bg-ink/80 flex items-center justify-center p-8 cursor-zoom-out"
          onClick={() => setZoomed(false)}
          role="dialog"
          aria-modal="true"
        >
          <img
            src={fullSrc}
            alt={`page ${page.page_number} (full)`}
            className="max-w-full max-h-full object-contain shadow-2xl"
            draggable={false}
            onClick={(e) => e.stopPropagation()}
          />
        </div>
      )}
    </>
  );
}
