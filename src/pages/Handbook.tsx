import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeft, ChevronLeft, ChevronRight, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useApp } from "@/state";
import { useToaster } from "@/components/Toaster";
import {
  games as gamesIpc,
  pages as pagesIpc,
  search as searchIpc,
  type Game,
  type Page,
  type SearchHit,
} from "@/lib/ipc";
import TocSidebar, { buildToc } from "@/components/handbook/TocSidebar";
import PageReader, {
  type PageReaderHandle,
} from "@/components/handbook/PageReader";
import SearchBar from "@/components/handbook/SearchBar";
import OriginalPageViewer from "@/components/handbook/OriginalPageViewer";

export default function Handbook() {
  const { t } = useTranslation();
  const { selectedGameId, setPage } = useApp();
  const toaster = useToaster();

  const [game, setGame] = useState<Game | null>(null);
  const [pageList, setPageList] = useState<Page[] | null>(null);
  const [activePageNumber, setActivePageNumber] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[] | null>(null);

  const readerRef = useRef<PageReaderHandle | null>(null);

  // Load game + pages when selectedGameId changes.
  useEffect(() => {
    if (!selectedGameId) {
      setGame(null);
      setPageList(null);
      return;
    }
    let cancelled = false;
    setGame(null);
    setPageList(null);
    Promise.all([
      gamesIpc.get(selectedGameId),
      pagesIpc.listByGame(selectedGameId),
    ])
      .then(([g, ps]) => {
        if (cancelled) return;
        setGame(g);
        setPageList(ps);
        if (ps.length > 0) setActivePageNumber(ps[0].page_number);
      })
      .catch((e) => {
        if (cancelled) return;
        toaster.push(String(e), "error");
        setGame(null);
        setPageList([]);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedGameId, toaster]);

  // Run keyword search when the (debounced) query changes.
  useEffect(() => {
    if (!selectedGameId) return;
    const q = query.trim();
    if (!q) {
      setHits(null);
      return;
    }
    let cancelled = false;
    setHits(null);
    searchIpc
      .keyword(q, selectedGameId, 20)
      .then((res) => {
        if (!cancelled) setHits(res);
      })
      .catch((e) => {
        if (!cancelled) {
          toaster.push(String(e), "error");
          setHits([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [query, selectedGameId, toaster]);

  const toc = useMemo(() => buildToc(pageList ?? []), [pageList]);

  const activePage = useMemo(() => {
    if (!pageList || activePageNumber === null) return null;
    return pageList.find((p) => p.page_number === activePageNumber) ?? null;
  }, [pageList, activePageNumber]);

  const totalPages = pageList?.length ?? 0;

  const handleJumpToPage = (n: number) => {
    setActivePageNumber(n);
    readerRef.current?.scrollToPage(n);
  };

  const handlePrev = () => {
    if (!pageList || pageList.length === 0 || activePageNumber === null) return;
    const idx = pageList.findIndex((p) => p.page_number === activePageNumber);
    if (idx > 0) handleJumpToPage(pageList[idx - 1].page_number);
  };

  const handleNext = () => {
    if (!pageList || pageList.length === 0 || activePageNumber === null) return;
    const idx = pageList.findIndex((p) => p.page_number === activePageNumber);
    if (idx >= 0 && idx < pageList.length - 1)
      handleJumpToPage(pageList[idx + 1].page_number);
  };

  // No game selected.
  if (!selectedGameId) {
    return (
      <section className="h-screen flex flex-col items-center justify-center px-10 gap-4">
        <p className="text-ink/60">先选一本规则书 / select a game</p>
        <Button onClick={() => setPage("library")}>
          <ArrowLeft className="w-4 h-4 mr-2" />
          {t("nav.library")}
        </Button>
      </section>
    );
  }

  // Loading.
  if (game === null || pageList === null) {
    return (
      <section className="h-screen flex items-center justify-center px-10">
        <p className="text-ink/60">{t("common.loading")}</p>
      </section>
    );
  }

  // No pages yet.
  if (pageList.length === 0) {
    return (
      <section className="h-screen flex flex-col items-center justify-center px-10 gap-4">
        <h1 className="font-handwritten text-4xl text-ink">{game.name_zh}</h1>
        <p className="text-ink/60">{t("handbook.noContent")}</p>
        <div className="flex gap-2">
          <Button variant="ghost" onClick={() => setPage("library")}>
            <ArrowLeft className="w-4 h-4 mr-2" />
            {t("nav.library")}
          </Button>
          <Button onClick={() => setPage("import", selectedGameId)}>
            添加页面
          </Button>
        </div>
      </section>
    );
  }

  const activeIdx =
    activePageNumber === null
      ? -1
      : pageList.findIndex((p) => p.page_number === activePageNumber);
  const displayIdx = activeIdx >= 0 ? activeIdx + 1 : 1;

  return (
    <section className="h-screen flex flex-col">
      {/* Top bar */}
      <header className="flex items-center gap-4 px-4 h-14 border-b border-ink/10 bg-paper shrink-0">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setPage("library")}
          aria-label={t("nav.library")}
        >
          <ArrowLeft className="w-4 h-4 mr-2" />
          <span className="text-ink/70">{t("nav.library")}</span>
          <span className="mx-2 text-ink/30">/</span>
          <span className="text-ink font-medium truncate max-w-[16rem]">
            {game.name_zh}
          </span>
        </Button>

        <div className="flex-1 flex items-center justify-center gap-2">
          <Button
            variant="ghost"
            size="sm"
            onClick={handlePrev}
            disabled={activeIdx <= 0}
            aria-label="prev page"
          >
            <ChevronLeft className="w-4 h-4" />
          </Button>
          <span className="text-sm text-ink/70 tabular-nums">
            {displayIdx} / {totalPages}
          </span>
          <Button
            variant="ghost"
            size="sm"
            onClick={handleNext}
            disabled={activeIdx < 0 || activeIdx >= totalPages - 1}
            aria-label="next page"
          >
            <ChevronRight className="w-4 h-4" />
          </Button>
        </div>

        <Button
          variant="ghost"
          size="sm"
          onClick={() => setPage("walkthrough", selectedGameId)}
          aria-label={t("walkthrough.title")}
          title={t("walkthrough.title")}
        >
          <Sparkles className="w-4 h-4 mr-1.5 text-accent" />
          <span>{t("walkthrough.title")}</span>
        </Button>

        <SearchBar
          value={query}
          onChange={setQuery}
          placeholder={t("handbook.searchPlaceholder")}
        />
      </header>

      {/* Three-column body */}
      <div className="flex-1 flex min-h-0">
        <TocSidebar
          gameTitle={game.name_zh}
          toc={toc}
          hits={hits}
          query={query}
          onJumpToPage={handleJumpToPage}
        />
        <PageReader
          ref={readerRef}
          pages={pageList}
          highlight={query}
          onActivePageChange={setActivePageNumber}
        />
        <OriginalPageViewer page={activePage} />
      </div>
    </section>
  );
}
