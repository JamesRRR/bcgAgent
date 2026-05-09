import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowLeft,
  BookOpen,
  Check,
  HelpCircle,
  RefreshCw,
  Send,
  Sparkles,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { Mic } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useApp } from "@/state";
import { useToaster } from "@/components/Toaster";
import {
  audio,
  games as gamesIpc,
  walkthrough,
  walkthroughSession,
  type Game,
  type WalkthroughTurn,
} from "@/lib/ipc";
import MarkdownView from "@/components/handbook/MarkdownView";
import { usePushToTalk } from "@/hooks/usePushToTalk";

// Strip markdown so TTS doesn't read "##" as "井号井号".
function stripMarkdownForTts(s: string): string {
  return s
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/\*([^*]+)\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/^[-*]\s+/gm, "")
    .replace(/^\d+\.\s+/gm, "")
    .replace(/\n{3,}/g, "\n\n");
}

// The agent wraps each instruction in `<<PHASE:foo>>` + `<<INSTRUCTION>>...
// <<END>>`. Strip those markers so the bubble shows just the prose.
function stripCoachMarkers(s: string): string {
  return s
    .replace(/<<PHASE:[^>]*>>/g, "")
    .replace(/<<INSTRUCTION>>/g, "")
    .replace(/<<END>>/g, "")
    .trim();
}

type TtsRequest = {
  requestId: string;
  handle: string | null;
  cancelled: boolean;
};

export default function Walkthrough() {
  const { t, i18n } = useTranslation();
  const { selectedGameId, setPage } = useApp();
  const toaster = useToaster();

  const [game, setGame] = useState<Game | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [voiceEnabled, setVoiceEnabled] = useState(false);
  const [speaking, setSpeaking] = useState(false);

  // Conversational session state.
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [phase, setPhase] = useState<string>("setup");
  const [turns, setTurns] = useState<WalkthroughTurn[]>([]);
  // Pending agent stream — buffered into a string until <<done>> fires, at
  // which point it's flushed into `turns` and cleared here.
  const [streamingAgent, setStreamingAgent] = useState<string>("");
  const streamingRef = useRef<string>("");
  const [thinking, setThinking] = useState(false);
  const [composerOpen, setComposerOpen] = useState(false);
  const [composer, setComposer] = useState("");
  const [showFullGuide, setShowFullGuide] = useState(false);
  const [fullGuideText, setFullGuideText] = useState("");
  const [fullGuideLoading, setFullGuideLoading] = useState(false);

  const ttsReqRef = useRef<TtsRequest | null>(null);
  const voiceEnabledRef = useRef(false);
  useEffect(() => {
    voiceEnabledRef.current = voiceEnabled;
  }, [voiceEnabled]);

  const scrollerRef = useRef<HTMLDivElement | null>(null);

  // ---------- TTS plumbing (kept tight; same race-fix as before) ----------

  const cancelCurrentTts = useCallback(() => {
    const cur = ttsReqRef.current;
    if (!cur) return;
    cur.cancelled = true;
    if (cur.handle) {
      const h = cur.handle;
      cur.handle = null;
      audio.speakCancel(h).catch(() => {});
    }
    ttsReqRef.current = null;
    setSpeaking(false);
  }, []);

  const playSpeech = useCallback(
    async (text: string) => {
      if (!voiceEnabledRef.current) return;
      const lang: "zh" | "en" = i18n.language === "zh-CN" ? "zh" : "en";
      const clean = stripMarkdownForTts(text);
      if (!clean.trim()) return;
      cancelCurrentTts();
      const req: TtsRequest = {
        requestId: Math.random().toString(36).slice(2),
        handle: null,
        cancelled: false,
      };
      ttsReqRef.current = req;
      setSpeaking(true);
      try {
        const handle = await audio.speak(clean, lang);
        if (req.cancelled) {
          audio.speakCancel(handle).catch(() => {});
          if (ttsReqRef.current === req) ttsReqRef.current = null;
          setSpeaking(false);
          return;
        }
        if (ttsReqRef.current === req) {
          req.handle = handle;
        } else {
          audio.speakCancel(handle).catch(() => {});
        }
      } catch (e) {
        if (ttsReqRef.current === req) ttsReqRef.current = null;
        setSpeaking(false);
        console.warn("walkthrough TTS failed:", e);
        toaster.push(String(e), "error");
      }
    },
    [i18n.language, toaster, cancelCurrentTts],
  );

  // Listen for natural TTS exit so the speaking indicator clears.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    audio
      .onTtsDone(({ handle_id }) => {
        const cur = ttsReqRef.current;
        if (cur && cur.handle === handle_id) {
          ttsReqRef.current = null;
          setSpeaking(false);
        }
      })
      .then((u) => {
        unlisten = u;
      });
    return () => unlisten?.();
  }, []);

  // Cancel TTS on unmount.
  useEffect(() => {
    return () => {
      cancelCurrentTts();
    };
  }, [cancelCurrentTts]);

  // ---------- Session loading + streaming listeners ----------

  // Auto-scroll to bottom when turns or streaming buffer change.
  useEffect(() => {
    const el = scrollerRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [turns, streamingAgent]);

  // Mount-time: register stream listeners. Each fires when ANY session
  // produces tokens, but we filter by `sessionId`.
  useEffect(() => {
    let unsubs: UnlistenFn[] = [];
    let active = true;
    Promise.all([
      walkthroughSession.onToken(({ session_id, token }) => {
        // Only accept tokens for the currently-displayed session.
        if (!active) return;
        const cur = sessionIdRef.current;
        if (cur && cur === session_id) {
          streamingRef.current += token;
          setStreamingAgent(streamingRef.current);
        }
      }),
      walkthroughSession.onDone(({ session_id, turn_no, phase: nextPhase, full_content }) => {
        if (!active) return;
        const cur = sessionIdRef.current;
        if (!cur || cur !== session_id) return;
        // Append the completed agent turn into history.
        setTurns((prev) => [
          ...prev,
          {
            turn_no,
            role: "agent",
            kind: "instruction",
            content: full_content,
            created_at: Math.floor(Date.now() / 1000),
          },
        ]);
        setStreamingAgent("");
        streamingRef.current = "";
        setPhase(nextPhase);
        setThinking(false);

        // Auto-narrate the new bubble if voice is on.
        if (voiceEnabledRef.current) {
          playSpeech(stripCoachMarkers(full_content));
        }
      }),
    ]).then((arr) => {
      if (!active) {
        arr.forEach((u) => u());
        return;
      }
      unsubs = arr;
    });
    return () => {
      active = false;
      unsubs.forEach((u) => u());
    };
  }, [playSpeech]);

  // Keep a ref for the listener closure.
  const sessionIdRef = useRef<string | null>(null);
  useEffect(() => {
    sessionIdRef.current = sessionId;
  }, [sessionId]);

  // Initial load: game metadata + cached session (resume) + cached one-shot guide.
  useEffect(() => {
    if (!selectedGameId) return;
    let cancelled = false;
    Promise.all([
      gamesIpc.get(selectedGameId),
      walkthroughSession.get(selectedGameId).catch(() => null),
    ]).then(([g, view]) => {
      if (cancelled) return;
      setGame(g);
      if (view) {
        setSessionId(view.session.session_id);
        setPhase(view.session.phase);
        setTurns(view.turns);
      } else {
        setSessionId(null);
        setTurns([]);
        setPhase("setup");
      }
      setErrorMsg(null);
    });
    return () => {
      cancelled = true;
    };
  }, [selectedGameId]);

  // ---------- Actions ----------

  const startCoaching = useCallback(async () => {
    if (!selectedGameId || thinking) return;
    setThinking(true);
    setErrorMsg(null);
    try {
      const view = await walkthroughSession.start(selectedGameId);
      setSessionId(view.session.session_id);
      setPhase(view.session.phase);
      setTurns(view.turns);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setErrorMsg(msg);
      toaster.push(msg, "error");
    } finally {
      setThinking(false);
    }
  }, [selectedGameId, thinking, toaster]);

  const sendUserTurn = useCallback(
    async (kind: "confirm" | "question", text: string) => {
      if (!sessionId || thinking) return;
      // Optimistically render the user turn.
      setTurns((prev) => [
        ...prev,
        {
          turn_no: (prev[prev.length - 1]?.turn_no ?? -1) + 1,
          role: "user",
          kind,
          content: text,
          created_at: Math.floor(Date.now() / 1000),
        },
      ]);
      setThinking(true);
      streamingRef.current = "";
      setStreamingAgent("");
      try {
        await walkthroughSession.continue_(sessionId, kind, text);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setErrorMsg(msg);
        toaster.push(msg, "error");
        setThinking(false);
      }
    },
    [sessionId, thinking, toaster],
  );

  const handleConfirm = () => {
    sendUserTurn("confirm", isZh ? "好了" : "Done");
  };

  const isZh = i18n.language === "zh-CN";

  const handleAskQuestion = () => {
    const q = composer.trim();
    if (!q) return;
    setComposer("");
    setComposerOpen(false);
    sendUserTurn("question", q);
  };

  const ptt = usePushToTalk({
    lang: i18n.language === "zh-CN" ? "zh" : "en",
    disabled: thinking || !sessionId,
    onPartial: (text) => {
      // Show live transcript in the composer so the user sees the recognizer
      // working. Open the composer if not already so the partial is visible.
      setComposerOpen(true);
      setComposer(text);
    },
    onFinalized: (text) => {
      setComposer("");
      setComposerOpen(false);
      sendUserTurn("question", text);
    },
    onError: (err) => toaster.push(err, "error"),
  });

  const handleReset = useCallback(async () => {
    if (!selectedGameId) return;
    cancelCurrentTts();
    try {
      await walkthroughSession.reset(selectedGameId);
      setSessionId(null);
      setTurns([]);
      setPhase("setup");
      setStreamingAgent("");
      streamingRef.current = "";
      setErrorMsg(null);
    } catch (e) {
      toaster.push(String(e), "error");
    }
  }, [selectedGameId, cancelCurrentTts, toaster]);

  const toggleVoice = useCallback(() => {
    setVoiceEnabled((v) => {
      const next = !v;
      if (!next) {
        cancelCurrentTts();
      }
      return next;
    });
  }, [cancelCurrentTts]);

  // ---------- Full-guide drawer (the old one-shot) ----------

  const openFullGuide = useCallback(async () => {
    if (!selectedGameId) return;
    setShowFullGuide(true);
    if (fullGuideText) return;
    setFullGuideLoading(true);
    try {
      const cached = await walkthrough.getCached(selectedGameId).catch(() => null);
      if (cached) {
        setFullGuideText(cached);
        setFullGuideLoading(false);
        return;
      }
      // No cache → generate now (uses the existing one-shot prompt).
      const result = await walkthrough.run(selectedGameId);
      setFullGuideText(result);
    } catch (e) {
      toaster.push(String(e), "error");
    } finally {
      setFullGuideLoading(false);
    }
  }, [selectedGameId, fullGuideText, toaster]);

  // ---------- Render ----------

  const phaseLabel = useMemo(() => {
    switch (phase) {
      case "first_round":
        return t("walkthrough.session.phaseFirstRound");
      case "midgame":
        return t("walkthrough.session.phaseMidgame");
      case "endgame":
        return t("walkthrough.session.phaseEndgame");
      default:
        return t("walkthrough.session.phaseSetup");
    }
  }, [phase, t]);

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

  const hasSession = !!sessionId && turns.length > 0;
  const lastTurn = turns[turns.length - 1];
  const canConfirm =
    hasSession && !thinking && !streamingAgent && lastTurn?.role === "agent";

  return (
    <section className="h-screen flex flex-col">
      <header className="flex items-center gap-2 px-4 h-14 border-b border-ink/10 bg-paper shrink-0">
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

        {hasSession && (
          <span className="ml-2 text-xs px-2 py-0.5 rounded-full bg-accent/10 text-accent border border-accent/20">
            {phaseLabel}
          </span>
        )}

        <div className="flex-1" />

        <Button
          variant="ghost"
          size="sm"
          onClick={openFullGuide}
          title={t("walkthrough.session.showFullGuide") as string}
        >
          <BookOpen className="w-4 h-4" />
        </Button>

        <Button
          variant="ghost"
          size="sm"
          onClick={toggleVoice}
          aria-pressed={voiceEnabled}
          aria-label={
            voiceEnabled
              ? t("walkthrough.voiceOff")
              : t("walkthrough.voiceOn")
          }
          title={
            voiceEnabled
              ? t("walkthrough.voiceOff")
              : t("walkthrough.voiceOn")
          }
          className={voiceEnabled ? "text-accent" : "text-ink/60"}
        >
          {voiceEnabled ? (
            <Volume2 className={`w-4 h-4 ${speaking ? "animate-pulse" : ""}`} />
          ) : (
            <VolumeX className="w-4 h-4" />
          )}
        </Button>

        {hasSession && (
          <Button
            variant="ghost"
            size="sm"
            onClick={handleReset}
            title={t("walkthrough.session.reset") as string}
          >
            <RefreshCw className="w-4 h-4" />
          </Button>
        )}
      </header>

      {/* ----- Chat body ----- */}
      <div ref={scrollerRef} className="flex-1 overflow-y-auto bg-cream/30">
        <div className="max-w-3xl mx-auto px-6 py-6">
          {!hasSession && !thinking && (
            <div className="rounded-md bg-paper p-8 border border-ink/10 text-ink/70">
              <h1 className="text-2xl font-handwritten text-ink mb-2">
                {t("walkthrough.title")}
                {game && (
                  <span className="text-ink/50 text-base font-normal ml-2">
                    《{game.name_zh}》
                  </span>
                )}
              </h1>
              <p className="mb-4">{t("walkthrough.intro")}</p>
              <Button onClick={startCoaching} disabled={thinking}>
                <Sparkles className="w-4 h-4 mr-2" />
                {thinking
                  ? t("walkthrough.session.starting")
                  : t("walkthrough.session.start")}
              </Button>
              {errorMsg && (
                <div
                  role="alert"
                  className="mt-4 p-3 rounded-md bg-rose-50 border border-rose-200 text-sm text-rose-900"
                >
                  <div className="font-medium mb-1">{t("common.error")}</div>
                  <div className="text-rose-800/90 break-words">{errorMsg}</div>
                  <div className="text-xs text-rose-700/70 mt-2">
                    {t("walkthrough.errorHint")}
                  </div>
                </div>
              )}
            </div>
          )}

          {hasSession && (
            <ul className="space-y-4">
              {turns.map((turn) => (
                <TurnBubble
                  key={turn.turn_no}
                  turn={turn}
                  isZh={isZh}
                  t={t}
                />
              ))}
              {streamingAgent && (
                <li className="flex">
                  <div className="max-w-[85%] rounded-2xl rounded-bl-sm bg-paper border border-ink/10 px-4 py-3 text-ink shadow-sm">
                    <div className="text-xs text-accent mb-1">
                      {t("walkthrough.session.coach")}
                    </div>
                    <MarkdownView source={stripCoachMarkers(streamingAgent)} />
                    <span className="inline-block w-2 h-4 bg-accent animate-pulse align-middle ml-1" />
                  </div>
                </li>
              )}
              {thinking && !streamingAgent && (
                <li className="flex">
                  <div className="max-w-[85%] rounded-2xl rounded-bl-sm bg-paper border border-ink/10 px-4 py-3 text-ink/50 italic shadow-sm">
                    {t("walkthrough.session.thinking")}
                  </div>
                </li>
              )}
            </ul>
          )}

          {errorMsg && hasSession && (
            <div
              role="alert"
              className="mt-4 p-3 rounded-md bg-rose-50 border border-rose-200 text-sm text-rose-900"
            >
              <div className="font-medium mb-1">{t("common.error")}</div>
              <div className="text-rose-800/90 break-words">{errorMsg}</div>
            </div>
          )}
        </div>
      </div>

      {/* ----- Footer (sticky composer) ----- */}
      {hasSession && (
        <footer className="border-t border-ink/10 bg-paper px-4 py-3 shrink-0">
          <div className="max-w-3xl mx-auto">
            {!composerOpen ? (
              <div className="space-y-2">
                <div className="flex gap-2">
                  <Button
                    onClick={handleConfirm}
                    disabled={!canConfirm}
                    className="flex-1"
                  >
                    <Check className="w-4 h-4 mr-2" />
                    {t("walkthrough.session.ready")}
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() => setComposerOpen(true)}
                    disabled={thinking}
                  >
                    <HelpCircle className="w-4 h-4 mr-2" />
                    {t("walkthrough.session.askInstead")}
                  </Button>
                </div>
                <div
                  className={
                    "flex items-center justify-center gap-2 text-xs " +
                    (ptt.state === "recording"
                      ? "text-accent"
                      : ptt.state === "transcribing"
                        ? "text-ink/70"
                        : "text-ink/40")
                  }
                >
                  <Mic
                    className={
                      "w-3.5 h-3.5 " +
                      (ptt.state === "recording" ? "animate-pulse" : "")
                    }
                  />
                  <span>
                    {ptt.state === "recording"
                      ? isZh
                        ? "正在录音…松开空格发送"
                        : "Recording… release Space to send"
                      : ptt.state === "transcribing"
                        ? isZh
                          ? "识别中…"
                          : "Transcribing…"
                        : isZh
                          ? "长按空格说话"
                          : "Hold Space to talk"}
                  </span>
                </div>
              </div>
            ) : (
              <div className="flex gap-2 items-end">
                <textarea
                  autoFocus
                  value={composer}
                  onChange={(e) => setComposer(e.target.value)}
                  placeholder={t("walkthrough.session.askPlaceholder") as string}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                      e.preventDefault();
                      handleAskQuestion();
                    }
                  }}
                  rows={2}
                  className="flex-1 rounded-md border border-ink/15 bg-cream/40 px-3 py-2 text-sm text-ink outline-none focus:border-accent resize-none"
                />
                <Button
                  onClick={handleAskQuestion}
                  disabled={!composer.trim() || thinking}
                >
                  <Send className="w-4 h-4 mr-1" />
                  {t("walkthrough.session.send")}
                </Button>
                <Button
                  variant="ghost"
                  onClick={() => {
                    setComposer("");
                    setComposerOpen(false);
                  }}
                >
                  <X className="w-4 h-4" />
                </Button>
              </div>
            )}
          </div>
        </footer>
      )}

      {/* ----- Full-guide drawer ----- */}
      {showFullGuide && (
        <div
          className="fixed inset-0 z-50 bg-black/30 flex items-center justify-center p-6"
          onClick={() => setShowFullGuide(false)}
        >
          <div
            className="bg-paper border border-ink/10 rounded-md max-w-2xl w-full max-h-[85vh] overflow-hidden flex flex-col"
            onClick={(e) => e.stopPropagation()}
          >
            <header className="flex items-center justify-between px-4 h-12 border-b border-ink/10">
              <h2 className="text-base font-medium text-ink">
                {t("walkthrough.session.showFullGuide")}
              </h2>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setShowFullGuide(false)}
              >
                <X className="w-4 h-4" />
              </Button>
            </header>
            <div className="flex-1 overflow-y-auto px-6 py-4">
              {fullGuideLoading ? (
                <p className="text-ink/50 italic">
                  {t("walkthrough.generating")}…
                </p>
              ) : (
                <MarkdownView source={fullGuideText || t("walkthrough.intro")} />
              )}
            </div>
          </div>
        </div>
      )}
    </section>
  );
}

// Single-turn bubble (agent or user). Memo would help on long sessions but
// the size of typical playthroughs (~30 turns) doesn't warrant it yet.
function TurnBubble({
  turn,
  t,
}: {
  turn: WalkthroughTurn;
  isZh: boolean;
  t: (k: string) => string;
}) {
  const isAgent = turn.role === "agent";
  const content = isAgent ? stripCoachMarkers(turn.content) : turn.content;
  return (
    <li className={`flex ${isAgent ? "" : "justify-end"}`}>
      <div
        className={`max-w-[85%] rounded-2xl px-4 py-3 shadow-sm border ${
          isAgent
            ? "rounded-bl-sm bg-paper border-ink/10 text-ink"
            : "rounded-br-sm bg-accent/10 border-accent/20 text-ink"
        }`}
      >
        <div className={`text-xs mb-1 ${isAgent ? "text-accent" : "text-accent/80"}`}>
          {isAgent
            ? t("walkthrough.session.coach")
            : t("walkthrough.session.you")}
        </div>
        {isAgent ? (
          <MarkdownView source={content} />
        ) : (
          <p className="whitespace-pre-wrap">{content}</p>
        )}
      </div>
    </li>
  );
}
