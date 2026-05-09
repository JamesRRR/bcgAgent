import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { ingest, research as researchIpc } from "@/lib/ipc";
import type { PageItem, PageStatus } from "@/components/import/PageCard";
import { useApp } from "@/state";
import { useToaster } from "@/components/Toaster";

type IngestState = {
  gameId: string | null;
  items: PageItem[];
  running: boolean;
  succeeded: number;
  failed: number;
  // Wave 4: post-import seed-crawl banner state.
  seedCrawl: SeedCrawlState | null;
};

/// `running` covers the optimistic "补充资料中…" banner. Once the backend
/// fires `seed_crawl:done`, we swap to `done` for 5 seconds before clearing.
type SeedCrawlState =
  | { kind: "running" }
  | { kind: "done"; chunks_added: number };

type IngestCtx = {
  state: IngestState;
  setItems: (updater: (cur: PageItem[]) => PageItem[]) => void;
  setGameId: (id: string | null) => void;
  start: (gameId: string, paths: string[]) => Promise<void>;
  retry: (gameId: string, item: PageItem) => Promise<void>;
  /// Manually dismiss the seed-crawl banner.
  dismissSeedCrawl: () => void;
};

const Ctx = createContext<IngestCtx | null>(null);

export function useIngestCtx(): IngestCtx {
  const v = useContext(Ctx);
  if (!v) throw new Error("useIngestCtx must be inside <IngestProvider>");
  return v;
}

let nextItemId = 0;
const makeItem = (path: string): PageItem => ({
  id: `pi-${++nextItemId}`,
  path,
  status: { kind: "pending" },
});
export { makeItem };

export default function IngestProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const { page, setPage } = useApp();
  const toaster = useToaster();

  const [state, setState] = useState<IngestState>({
    gameId: null,
    items: [],
    running: false,
    succeeded: 0,
    failed: 0,
    seedCrawl: null,
  });

  // The path-array as submitted to the running job, indexed by page_number-1.
  // Events carry page_number, not path, so we look up here to update items.
  const pathsAtRunRef = useRef<string[]>([]);
  // Track current page so the onDone handler can decide whether to auto-nav.
  // Using a ref so the listener doesn't need to re-register on page change.
  const pageRef = useRef(page);
  useEffect(() => {
    pageRef.current = page;
  }, [page]);

  const updateStatusByPageNumber = useCallback(
    (pageNumber: number, status: PageStatus) => {
      const path = pathsAtRunRef.current[pageNumber - 1];
      if (!path) return;
      setState((cur) => ({
        ...cur,
        items: cur.items.map((it) =>
          it.path === path ? { ...it, status } : it,
        ),
      }));
    },
    [],
  );

  // Register listeners ONCE for the lifetime of the app — they survive
  // every navigation. Uses Tauri's invoke under inTauri, HTTP/SSE shim otherwise.
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];
    (async () => {
      const us = await Promise.all([
        ingest.onPageStarted(({ page_number }) =>
          updateStatusByPageNumber(page_number, { kind: "running" }),
        ),
        ingest.onPageDone(({ page_number, chunk_count }) =>
          updateStatusByPageNumber(page_number, {
            kind: "done",
            chunkCount: chunk_count,
          }),
        ),
        ingest.onPageFailed(({ page_number, error }) =>
          updateStatusByPageNumber(page_number, { kind: "failed", error }),
        ),
        ingest.onDone(({ succeeded, failed, game_id }) => {
          setState((cur) => ({
            ...cur,
            running: false,
            succeeded,
            failed,
            // Optimistic banner: backend has just kicked off seed crawl.
            seedCrawl: succeeded > 0 ? { kind: "running" } : cur.seedCrawl,
          }));
          toaster.push(
            `${t("import.progress.done")} ${succeeded}/${succeeded + failed}`,
            failed > 0 ? "error" : "success",
          );
          // Only auto-navigate to Handbook if the user is still on Import —
          // otherwise respect that they navigated elsewhere.
          if (pageRef.current === "import") {
            setPage("handbook", game_id);
          }
        }),
        researchIpc.onSeedCrawlDone(({ chunks_added }) => {
          setState((cur) => ({
            ...cur,
            seedCrawl: { kind: "done", chunks_added },
          }));
          // Auto-clear after 5 seconds so the UI doesn't get stale.
          setTimeout(() => {
            setState((cur) => ({ ...cur, seedCrawl: null }));
          }, 5000);
        }),
      ]);
      if (cancelled) {
        us.forEach((u) => u());
      } else {
        unlisteners.push(...us);
      }
    })();
    return () => {
      cancelled = true;
      unlisteners.forEach((u) => u());
    };
  }, [updateStatusByPageNumber, setPage, t, toaster]);

  const setItems: IngestCtx["setItems"] = useCallback((updater) => {
    setState((cur) => ({ ...cur, items: updater(cur.items) }));
  }, []);

  const setGameId: IngestCtx["setGameId"] = useCallback((id) => {
    setState((cur) => ({ ...cur, gameId: id }));
  }, []);

  const start: IngestCtx["start"] = useCallback(async (gameId, paths) => {
    pathsAtRunRef.current = paths;
    setState((cur) => ({
      ...cur,
      gameId,
      running: true,
      succeeded: 0,
      failed: 0,
      items: cur.items.map((it) =>
        paths.includes(it.path) ? { ...it, status: { kind: "pending" } } : it,
      ),
    }));
    try {
      await ingest.run(gameId, paths);
    } catch (e) {
      setState((cur) => ({ ...cur, running: false }));
      throw e;
    }
  }, []);

  const retry: IngestCtx["retry"] = useCallback(async (gameId, item) => {
    // Single-page run: backend reports page_number=1, so we re-alias the
    // ref to a one-element array pointing at this item's path.
    pathsAtRunRef.current = [item.path];
    setState((cur) => ({
      ...cur,
      running: true,
      items: cur.items.map((i) =>
        i.id === item.id ? { ...i, status: { kind: "running" } } : i,
      ),
    }));
    try {
      await ingest.run(gameId, [item.path]);
    } catch (e) {
      setState((cur) => ({
        ...cur,
        running: false,
        items: cur.items.map((i) =>
          i.id === item.id
            ? { ...i, status: { kind: "failed", error: String(e) } }
            : i,
        ),
      }));
      throw e;
    }
  }, []);

  const dismissSeedCrawl = useCallback(() => {
    setState((cur) => ({ ...cur, seedCrawl: null }));
  }, []);

  return (
    <Ctx.Provider
      value={{ state, setItems, setGameId, start, retry, dismissSeedCrawl }}
    >
      {state.seedCrawl && <SeedCrawlBanner state={state.seedCrawl} onDismiss={dismissSeedCrawl} />}
      {children}
    </Ctx.Provider>
  );
}

function SeedCrawlBanner({
  state,
  onDismiss,
}: {
  state: SeedCrawlState;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  const text =
    state.kind === "running"
      ? t("ingest.seedCrawl.running")
      : t("ingest.seedCrawl.done", { count: state.chunks_added });
  return (
    <div
      data-testid="seed-crawl-banner"
      className="fixed bottom-6 right-6 z-50 flex items-center gap-3 rounded-md border border-ink/15 bg-paper px-4 py-2 text-sm text-ink shadow-md"
    >
      <span>{text}</span>
      <button
        type="button"
        onClick={onDismiss}
        aria-label={t("common.cancel")}
        className="text-ink/40 hover:text-ink"
      >
        ×
      </button>
    </div>
  );
}
