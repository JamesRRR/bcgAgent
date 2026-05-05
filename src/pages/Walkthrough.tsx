import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeft, RefreshCw, Sparkles } from "lucide-react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { Button } from "@/components/ui/button";
import { useApp } from "@/state";
import { useToaster } from "@/components/Toaster";
import { games as gamesIpc, walkthrough, type Game } from "@/lib/ipc";
import MarkdownView from "@/components/handbook/MarkdownView";

export default function Walkthrough() {
  const { t } = useTranslation();
  const { selectedGameId, setPage } = useApp();
  const toaster = useToaster();

  const [game, setGame] = useState<Game | null>(null);
  const [content, setContent] = useState("");
  const [streaming, setStreaming] = useState(false);
  const unlistenersRef = useRef<UnlistenFn[]>([]);

  useEffect(() => {
    if (!selectedGameId) return;
    let cancelled = false;
    gamesIpc.get(selectedGameId).then((g) => {
      if (!cancelled) setGame(g);
    });
    return () => {
      cancelled = true;
    };
  }, [selectedGameId]);

  useEffect(() => {
    return () => {
      for (const u of unlistenersRef.current) u();
      unlistenersRef.current = [];
    };
  }, []);

  const generate = async () => {
    if (!selectedGameId || streaming) return;
    setContent("");
    setStreaming(true);

    // Register listeners BEFORE invoking the command so we don't miss tokens.
    const unsubs: UnlistenFn[] = [];
    unsubs.push(
      await walkthrough.onToken((tok) => setContent((cur) => cur + tok)),
      await walkthrough.onDone(() => setStreaming(false)),
    );
    unlistenersRef.current.push(...unsubs);

    try {
      await walkthrough.run(selectedGameId);
    } catch (e) {
      toaster.push(String(e), "error");
      setStreaming(false);
    }
  };

  if (!selectedGameId) {
    return (
      <section className="h-screen flex flex-col items-center justify-center px-10 gap-4">
        <p className="text-ink/60">{t("walkthrough.pickGame")}</p>
        <Button onClick={() => setPage("library")}>
          <ArrowLeft className="w-4 h-4 mr-2" />
          {t("nav.library")}
        </Button>
      </section>
    );
  }

  return (
    <section className="h-screen flex flex-col">
      <header className="flex items-center gap-4 px-4 h-14 border-b border-ink/10 bg-paper shrink-0">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setPage("handbook", selectedGameId)}
        >
          <ArrowLeft className="w-4 h-4 mr-2" />
          <span className="text-ink/70">{t("handbook.title")}</span>
          {game && (
            <>
              <span className="mx-2 text-ink/30">/</span>
              <span className="text-ink font-medium">{game.name_zh}</span>
            </>
          )}
        </Button>
        <div className="flex-1" />
        <Button onClick={generate} disabled={streaming} size="sm">
          {content ? (
            <RefreshCw className="w-4 h-4 mr-2" />
          ) : (
            <Sparkles className="w-4 h-4 mr-2" />
          )}
          {streaming
            ? t("walkthrough.generating")
            : content
              ? t("walkthrough.regenerate")
              : t("walkthrough.generate")}
        </Button>
      </header>

      <div className="flex-1 overflow-y-auto bg-cream/30">
        <div className="max-w-3xl mx-auto px-8 py-8">
          <h1 className="text-3xl font-handwritten text-ink mb-2">
            {t("walkthrough.title")}
          </h1>
          {game && (
            <p className="text-ink/60 mb-6">《{game.name_zh}》</p>
          )}

          {!content && !streaming && (
            <div className="rounded-md bg-paper p-8 border border-ink/10 text-ink/70">
              <p className="mb-4">{t("walkthrough.intro")}</p>
              <Button onClick={generate}>
                <Sparkles className="w-4 h-4 mr-2" />
                {t("walkthrough.generate")}
              </Button>
            </div>
          )}

          {(content || streaming) && (
            <article className="rounded-md bg-paper p-8 border-t-4 border-accent shadow-sm">
              {content ? (
                <MarkdownView source={content} />
              ) : (
                <p className="text-ink/50 italic">
                  {t("walkthrough.generating")}…
                </p>
              )}
              {streaming && content && (
                <span className="inline-block w-2 h-5 bg-accent animate-pulse align-middle ml-1" />
              )}
            </article>
          )}
        </div>
      </div>
    </section>
  );
}
