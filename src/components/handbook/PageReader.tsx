import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import type { Page } from "@/lib/ipc";
import PageArticle from "./PageArticle";

export type PageReaderHandle = {
  scrollToPage: (pageNumber: number) => void;
};

type Props = {
  pages: Page[];
  highlight: string;
  onActivePageChange: (pageNumber: number) => void;
};

const PageReader = forwardRef<PageReaderHandle, Props>(function PageReader(
  { pages, highlight, onActivePageChange },
  ref,
) {
  const containerRef = useRef<HTMLDivElement | null>(null);

  useImperativeHandle(ref, () => ({
    scrollToPage(pageNumber: number) {
      const el = containerRef.current?.querySelector<HTMLElement>(
        `#page-${pageNumber}`,
      );
      if (!el) return;
      // If the user is searching and there's a highlighted match on this
      // page, jump to the first <mark> instead of the article top so the
      // hit is actually visible.
      const mark = el.querySelector<HTMLElement>("mark");
      const target: HTMLElement = mark ?? el;
      target.scrollIntoView({
        behavior: "smooth",
        block: mark ? "center" : "start",
      });
    },
  }));

  useEffect(() => {
    const root = containerRef.current;
    if (!root) return;
    const articles = Array.from(
      root.querySelectorAll<HTMLElement>("article[data-page-number]"),
    );
    if (articles.length === 0) return;

    // Track which articles are currently intersecting and pick the topmost.
    const visible = new Map<HTMLElement, number>(); // el -> intersectionRatio
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const el = entry.target as HTMLElement;
          if (entry.isIntersecting) {
            visible.set(el, entry.intersectionRatio);
          } else {
            visible.delete(el);
          }
        }
        if (visible.size === 0) return;
        // Pick the article with the smallest top offset (topmost visible).
        let topEl: HTMLElement | null = null;
        let topY = Number.POSITIVE_INFINITY;
        for (const el of visible.keys()) {
          const top = el.getBoundingClientRect().top;
          if (top < topY) {
            topY = top;
            topEl = el;
          }
        }
        if (topEl) {
          const n = Number(topEl.dataset.pageNumber);
          if (!Number.isNaN(n)) onActivePageChange(n);
        }
      },
      {
        root,
        // Top-band trigger: an article counts as active when its top crosses
        // the 30%-from-top line of the scroll container.
        rootMargin: "0px 0px -70% 0px",
        threshold: [0, 0.01, 0.5, 1],
      },
    );
    articles.forEach((a) => observer.observe(a));
    return () => observer.disconnect();
  }, [pages, onActivePageChange]);

  return (
    <div ref={containerRef} className="flex-1 overflow-y-auto bg-cream/30">
      <div className="max-w-3xl mx-auto px-8 py-8 space-y-8">
        {pages.map((p) => (
          <PageArticle
            key={p.id}
            page={p}
            highlight={highlight}
            externalSource={p.ocr_status === "external"}
          />
        ))}
      </div>
    </div>
  );
});

export default PageReader;
