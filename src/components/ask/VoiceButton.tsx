import { useEffect, useRef, useState } from "react";
import { motion } from "framer-motion";
import { Mic, Square } from "lucide-react";
import { audio } from "@/lib/ipc";
import { useToaster } from "@/components/Toaster";
import { blobToWav16k } from "./wav";
import { cn } from "@/lib/utils";

type Props = {
  disabled?: boolean;
  onTranscribed: (text: string) => void;
};

type State = "idle" | "recording" | "transcribing";

export default function VoiceButton({ disabled, onTranscribed }: Props) {
  const toaster = useToaster();
  const [state, setState] = useState<State>("idle");
  const recorderRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<BlobPart[]>([]);
  const streamRef = useRef<MediaStream | null>(null);

  // Stop / cleanup on unmount
  useEffect(() => {
    return () => {
      try {
        recorderRef.current?.stop();
      } catch {
        /* noop */
      }
      streamRef.current?.getTracks().forEach((t) => t.stop());
    };
  }, []);

  const startRecording = async () => {
    if (disabled || state !== "idle") return;
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      streamRef.current = stream;
      const rec = new MediaRecorder(stream);
      chunksRef.current = [];
      rec.ondataavailable = (e) => {
        if (e.data && e.data.size > 0) chunksRef.current.push(e.data);
      };
      rec.onstop = async () => {
        // Release mic
        streamRef.current?.getTracks().forEach((t) => t.stop());
        streamRef.current = null;

        const blob = new Blob(chunksRef.current, {
          type: rec.mimeType || "audio/webm",
        });
        chunksRef.current = [];
        if (blob.size === 0) {
          setState("idle");
          return;
        }
        setState("transcribing");
        try {
          const wav = await blobToWav16k(blob);
          const text = await audio.transcribe(wav, "auto");
          if (text.trim()) onTranscribed(text.trim());
        } catch (err) {
          toaster.push(String(err), "error");
        } finally {
          setState("idle");
        }
      };
      recorderRef.current = rec;
      rec.start();
      setState("recording");
    } catch {
      toaster.push("麦克风权限被拒绝 / Microphone permission denied", "error");
    }
  };

  const stopRecording = () => {
    const rec = recorderRef.current;
    if (rec && rec.state !== "inactive") {
      rec.stop();
    }
    recorderRef.current = null;
  };

  const handlePointerDown = (e: React.PointerEvent) => {
    e.preventDefault();
    void startRecording();
  };

  const handlePointerUp = () => {
    if (state === "recording") stopRecording();
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
