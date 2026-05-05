import type { Page, SearchHit } from "@/lib/ipc";

export type TocEntry = {
  level: number;
  text: string;
  pageNumber: number;
};

export function buildToc(pages: Page[]): TocEntry[] {
  const entries: TocEntry[] = [];
  for (const p of pages) {
    if (!p.ocr_markdown) continue;
    const lines = p.ocr_markdown.split(/\r?\n/);
    let inFence = false;
    for (const raw of lines) {
      // Skip code fences.
      if (/^\s*```/.test(raw)) {
        inFence = !inFence;
        continue;
      }
      if (inFence) continue;
      const m = raw.match(/^(#{1,3})\s+(.+?)\s*#*\s*$/);
      if (!m) continue;
      entries.push({
        level: m[1].length,
        text: m[2].trim(),
        pageNumber: p.page_number,
      });
    }
  }
  return entries;
}

type Props = {
  gameTitle: string;
  toc: TocEntry[];
  hits: SearchHit[] | null;
  query: string;
  onJumpToPage: (pageNumber: number) => void;
};

export default function TocSidebar({
  gameTitle,
  toc,
  hits,
  query,
  onJumpToPage,
}: Props) {
  const showHits = query.trim().length > 0;
  return (
    <aside className="w-[200px] shrink-0 border-r border-ink/10 bg-cream/40 overflow-y-auto">
      <div className="px-4 pt-6 pb-3 sticky top-0 bg-cream/80 backdrop-blur-sm">
        <h2 className="font-handwritten text-2xl text-ink leading-tight">
          {gameTitle}
        </h2>
      </div>
      <div className="px-2 pb-6">
        {showHits ? (
          <HitList hits={hits} onJumpToPage={onJumpToPage} />
        ) : (
          <TocList toc={toc} onJumpToPage={onJumpToPage} />
        )}
      </div>
    </aside>
  );
}

function TocList({
  toc,
  onJumpToPage,
}: {
  toc: TocEntry[];
  onJumpToPage: (n: number) => void;
}) {
  if (toc.length === 0) {
    return (
      <p className="px-3 py-4 text-xs text-ink/50">No headings yet.</p>
    );
  }
  return (
    <ul className="text-sm">
      {toc.map((e, i) => (
        <li key={i}>
          <button
            type="button"
            onClick={() => onJumpToPage(e.pageNumber)}
            className="w-full text-left py-1 px-2 rounded hover:bg-paper text-ink/85 hover:text-ink truncate"
            style={{ paddingLeft: `${(e.level - 1) * 12 + 8}px` }}
            title={e.text}
          >
            <span
              className={
                e.level === 1
                  ? "font-semibold"
                  : e.level === 2
                    ? "font-medium"
                    : "text-ink/70"
              }
            >
              {e.text}
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}

function HitList({
  hits,
  onJumpToPage,
}: {
  hits: SearchHit[] | null;
  onJumpToPage: (n: number) => void;
}) {
  if (hits === null) {
    return <p className="px-3 py-4 text-xs text-ink/50">Searching…</p>;
  }
  if (hits.length === 0) {
    return <p className="px-3 py-4 text-xs text-ink/50">No matches.</p>;
  }
  return (
    <ul className="text-xs space-y-1">
      {hits.map((h) => (
        <li key={h.chunk_id}>
          <button
            type="button"
            onClick={() => onJumpToPage(h.page_number)}
            className="w-full text-left py-2 px-2 rounded hover:bg-paper border border-transparent hover:border-ink/10"
          >
            <div className="flex items-center gap-1.5 mb-1">
              <span className="px-1.5 py-0.5 rounded bg-accent/15 text-accent text-[10px] font-medium">
                p.{h.page_number}
              </span>
              {h.heading_path && (
                <span className="text-ink/50 truncate">{h.heading_path}</span>
              )}
            </div>
            <p className="text-ink/75 line-clamp-3 leading-snug">{h.content}</p>
          </button>
        </li>
      ))}
    </ul>
  );
}
