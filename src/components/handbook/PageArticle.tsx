import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { pages as pagesIpc, type Page, type PageIllustration } from "@/lib/ipc";
import MarkdownView, { type IllustrationMap } from "./MarkdownView";

type Props = {
  page: Page;
  highlight: string;
  /** External handbook pages have no OCR — show the markdown plainly. */
  externalSource?: boolean;
};

/** One page article. Fetches its own illustration crops so the markdown can
 *  render `![label](ill:N)` anchors as inline figures. */
export default function PageArticle({ page, highlight, externalSource }: Props) {
  const { t } = useTranslation();
  const [illustrations, setIllustrations] = useState<PageIllustration[]>([]);

  useEffect(() => {
    if (externalSource) {
      setIllustrations([]);
      return;
    }
    let cancelled = false;
    pagesIpc
      .illustrations(page.id)
      .then((rows) => {
        if (!cancelled) setIllustrations(rows);
      })
      .catch(() => {
        if (!cancelled) setIllustrations([]);
      });
    return () => {
      cancelled = true;
    };
  }, [page.id, externalSource]);

  const illMap: IllustrationMap = {};
  for (const i of illustrations) {
    if (i.token) {
      illMap[i.token] = { image_path: i.image_path, label: i.label };
    }
  }

  return (
    <article
      key={page.id}
      id={`page-${page.page_number}`}
      data-page-number={page.page_number}
      className="rounded-md bg-paper p-8 shadow-sm border-t-4 scroll-mt-6"
      style={{ borderTopColor: "var(--accent, #C8553D)" }}
    >
      <header className="mb-4 flex items-baseline justify-between">
        <span className="font-handwritten text-3xl text-ink/40">
          p. {page.page_number}
        </span>
        {page.ocr_status !== "done" && page.ocr_status !== "external" && (
          <span
            className={`text-xs px-2 py-0.5 rounded-full ${
              page.ocr_status === "failed"
                ? "bg-accent/15 text-accent"
                : "bg-cream text-ink/60"
            }`}
          >
            {page.ocr_status === "failed" ? "OCR 失败" : "OCR 中…"}
          </span>
        )}
      </header>
      {(page.ocr_status === "done" || page.ocr_status === "external") &&
      page.ocr_markdown ? (
        <MarkdownView
          source={page.ocr_markdown}
          highlight={highlight}
          illustrations={illMap}
        />
      ) : (
        <p className="text-ink/40 italic">{t("handbook.noContent")}</p>
      )}
    </article>
  );
}
