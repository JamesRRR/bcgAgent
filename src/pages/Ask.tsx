import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Volume2, VolumeX } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useApp } from "@/state";
import { useToaster } from "@/components/Toaster";
import {
  ask as askIpc,
  audio,
  qa as qaIpc,
  type QAHistory,
  type RetrievedChunk,
} from "@/lib/ipc";
import GameFilter from "@/components/ask/GameFilter";
import AnswerCard from "@/components/ask/AnswerCard";
import AskBar from "@/components/ask/AskBar";
import HistoryList from "@/components/ask/HistoryList";

const TTS_KEY = "tts_on";

export default function Ask() {
  const { t, i18n } = useTranslation();
  const toaster = useToaster();
  const { selectedGameId } = useApp();

  const [gameFilter, setGameFilter] = useState<string | null>(
    selectedGameId ?? null,
  );
  const [ttsOn, setTtsOn] = useState<boolean>(() => {
    return localStorage.getItem(TTS_KEY) === "1";
  });
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [streaming, setStreaming] = useState(false);
  const [question, setQuestion] = useState<string | null>(null);
  const [answer, setAnswer] = useState("");
  const [citations, setCitations] = useState<RetrievedChunk[]>([]);
  const [history, setHistory] = useState<QAHistory[]>([]);

  // Track active TTS handle so we can cancel.
  const ttsHandleRef = useRef<string | null>(null);
  // Latest answer text captured at stream-completion time, so we can
  // send it to TTS without waiting for React state to flush.
  const finalAnswerRef = useRef<string>("");

  const refreshHistory = useCallback(
    (gid: string | null) => {
      qaIpc
        .list(gid, 30)
        .then(setHistory)
        .catch((e) => toaster.push(String(e), "error"));
    },
    [toaster],
  );

  useEffect(() => {
    refreshHistory(gameFilter);
  }, [gameFilter, refreshHistory]);

  useEffect(() => {
    localStorage.setItem(TTS_KEY, ttsOn ? "1" : "0");
  }, [ttsOn]);

  const handleAsk = useCallback(
    async (q: string) => {
      if (busy) return;
      setBusy(true);
      setStreaming(true);
      setQuestion(q);
      setAnswer("");
      setCitations([]);
      setInput("");
      finalAnswerRef.current = "";

      // Cancel any in-flight TTS before starting a new ask.
      if (ttsHandleRef.current) {
        const h = ttsHandleRef.current;
        ttsHandleRef.current = null;
        audio.speakCancel(h).catch(() => {
          /* noop */
        });
      }

      // Register listeners BEFORE invoking ask.run().
      const unsubs: Array<() => void> = [];
      const unC = await askIpc.onCitations((chunks) => {
        setCitations(chunks);
      });
      unsubs.push(unC);
      const unT = await askIpc.onToken((tok) => {
        finalAnswerRef.current += tok;
        setAnswer((prev) => prev + tok);
      });
      unsubs.push(unT);
      const unD = await askIpc.onDone(() => {
        setStreaming(false);
        setBusy(false);
        unsubs.forEach((u) => u());
        refreshHistory(gameFilter);
        if (ttsOn && finalAnswerRef.current.trim()) {
          const lang: "zh" | "en" =
            i18n.language === "zh-CN" ? "zh" : "en";
          audio
            .speak(finalAnswerRef.current, lang)
            .then((handle) => {
              ttsHandleRef.current = handle;
            })
            .catch((e) => toaster.push(String(e), "error"));
        }
      });
      unsubs.push(unD);

      try {
        await askIpc.run(q, gameFilter);
      } catch (err) {
        const msg = String(err);
        unsubs.forEach((u) => u());
        setStreaming(false);
        setBusy(false);
        if (msg.includes("MissingKey")) {
          toaster.push(
            i18n.language === "zh-CN"
              ? "请先在 设置 中配置 MiniMax / DashScope 密钥"
              : "Please configure API keys in Settings first",
            "error",
          );
        } else {
          toaster.push(msg, "error");
        }
      }
    },
    [busy, gameFilter, i18n.language, refreshHistory, toaster, ttsOn],
  );

  // Cleanup on unmount: cancel TTS if any.
  useEffect(() => {
    return () => {
      if (ttsHandleRef.current) {
        audio.speakCancel(ttsHandleRef.current).catch(() => {
          /* noop */
        });
      }
    };
  }, []);

  const toggleTts = () => {
    if (ttsOn && ttsHandleRef.current) {
      const h = ttsHandleRef.current;
      ttsHandleRef.current = null;
      audio.speakCancel(h).catch(() => {
        /* noop */
      });
    }
    setTtsOn((v) => !v);
  };

  const pickHistory = (item: QAHistory) => {
    setQuestion(item.question);
    setAnswer(item.answer ?? "");
    setCitations([]);
    setStreaming(false);
  };

  return (
    <section className="flex h-screen">
      <div className="flex-1 min-w-0 flex flex-col px-10 py-8">
        <header className="flex items-center justify-between mb-6">
          <div className="flex items-center gap-3">
            <h1 className="text-2xl font-semibold">{t("ask.title")}</h1>
            <GameFilter value={gameFilter} onChange={setGameFilter} />
          </div>
          <Button
            variant={ttsOn ? "default" : "ghost"}
            size="sm"
            onClick={toggleTts}
            className="gap-1.5"
            aria-label={ttsOn ? t("ask.stopTTS") : t("ask.playTTS")}
          >
            {ttsOn ? (
              <Volume2 className="w-4 h-4" />
            ) : (
              <VolumeX className="w-4 h-4" />
            )}
          </Button>
        </header>

        <div className="flex-1 min-h-0 overflow-y-auto pr-2">
          <AnswerCard
            question={question}
            answer={answer}
            citations={citations}
            streaming={streaming}
          />
        </div>

        <div className="pt-6">
          <AskBar
            busy={busy}
            value={input}
            onChange={setInput}
            onSubmit={handleAsk}
          />
        </div>
      </div>
      <HistoryList items={history} onPick={pickHistory} />
    </section>
  );
}
