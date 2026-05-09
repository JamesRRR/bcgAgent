import { useEffect, useRef, useState } from "react";
import { motion } from "framer-motion";
import { Mic, Square } from "lucide-react";
import { audio } from "@/lib/ipc";
import { useToaster } from "@/components/Toaster";
import { cn } from "@/lib/utils";

type Props = {
  disabled?: boolean;
  onTranscribed: (text: string) => void;
};

type State = "idle" | "recording" | "transcribing";

/**
 * Press-and-hold-to-record button.
 *
 * Backed by native cpal mic capture in Rust (not browser getUserMedia) so the
 * app actually triggers the macOS TCC mic-permission prompt and shows up in
 * System Settings → Privacy → Microphone. WKWebView denies getUserMedia
 * silently, which is why the previous implementation always errored.
 */
export default function VoiceButton({ disabled, onTranscribed }: Props) {
  const toaster = useToaster();
  const [state, setStateRaw] = useState<State>("idle");
  const stateRef = useRef<State>("idle");
  const setState = (s: State) => {
    stateRef.current = s;
    setStateRaw(s);
  };
  const sessionIdRef = useRef<string | null>(null);

  // Subscribe to live partials so the button could expose them later if we
  // wanted to. For now the Ask flow only uses the final transcript.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    audio
      .onTranscribePartial(() => {
        // No-op — Ask page surfaces final transcript only.
      })
      .then((u) => {
        unlisten = u;
      });
    return () => unlisten?.();
  }, []);

  // Cancel any active session if the component unmounts mid-record.
  useEffect(() => {
    return () => {
      const sid = sessionIdRef.current;
      sessionIdRef.current = null;
      if (sid) audio.micCaptureCancel(sid).catch(() => {});
    };
  }, []);

  const startRecording = async () => {
    if (disabled || stateRef.current !== "idle") return;
    const sessionId = crypto.randomUUID();
    sessionIdRef.current = sessionId;
    // Flip UI immediately so the user sees feedback. The backend command
    // does a one-time whisper-model download (~1-2 min) on first use, and
    // without this optimistic update the button looks frozen.
    setState("recording");
    try {
      await audio.micCaptureStart(sessionId, "auto");
    } catch (err) {
      sessionIdRef.current = null;
      setState("idle");
      toaster.push(String(err), "error");
    }
  };

  const stopRecording = async () => {
    const sid = sessionIdRef.current;
    sessionIdRef.current = null;
    if (!sid) {
      setState("idle");
      return;
    }
    setState("transcribing");
    try {
      const text = await audio.micCaptureStop(sid);
      if (text.trim()) onTranscribed(text.trim());
    } catch (err) {
      toaster.push(String(err), "error");
    } finally {
      setState("idle");
    }
  };

  const handlePointerDown = (e: React.PointerEvent) => {
    e.preventDefault();
    void startRecording();
  };

  const handlePointerUp = () => {
    if (stateRef.current === "recording") void stopRecording();
  };

  return (
    <button
      type="button"
      disabled={disabled || state === "transcribing"}
      onPointerDown={handlePointerDown}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerUp}
      onPointerLeave={handlePointerUp}
      className={cn(
        "relative w-16 h-16 rounded-full flex items-center justify-center select-none transition-colors disabled:opacity-50",
        state === "idle" && "bg-paper border-2 border-accent text-accent",
        state === "recording" && "bg-accent text-cream",
        state === "transcribing" && "bg-accent/40 text-cream",
      )}
      aria-label="record"
    >
      {state === "idle" && (
        <motion.span
          className="absolute inset-0 rounded-full"
          animate={{ scale: [1, 1.05, 1] }}
          transition={{ duration: 2, repeat: Infinity, ease: "easeInOut" }}
        />
      )}
      {state === "recording" && (
        <motion.span
          className="absolute inset-0 rounded-full bg-accent"
          animate={{ scale: [1, 1.3], opacity: [0.5, 0] }}
          transition={{ duration: 1.2, repeat: Infinity, ease: "easeOut" }}
        />
      )}
      <span className="relative z-10">
        {state === "idle" && <Mic className="w-6 h-6" />}
        {state === "recording" && <Square className="w-5 h-5" />}
        {state === "transcribing" && (
          <span className="block w-5 h-5 border-2 border-cream border-t-transparent rounded-full animate-spin" />
        )}
      </span>
    </button>
  );
}
