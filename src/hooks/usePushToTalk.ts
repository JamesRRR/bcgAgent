import { useCallback, useEffect, useRef, useState } from "react";
import { audio, type LangHint } from "@/lib/ipc";

type State = "idle" | "recording" | "transcribing";

type Options = {
  /** Lang hint passed to whisper. */
  lang: LangHint;
  /** Disable while another async op is running (e.g. agent streaming). */
  disabled?: boolean;
  /** Window where partial transcript shows live. Updated as whisper runs. */
  onPartial?: (text: string) => void;
  /** Called once with the final transcript when the user releases the key. */
  onFinalized: (text: string) => void;
  /** Optional: shown to user when mic permission is denied or recorder fails. */
  onError?: (err: string) => void;
};

/**
 * Hold-Space-to-talk hook backed by native cpal mic capture.
 *
 * Why not getUserMedia: WKWebView on macOS denies media-capture permission
 * silently and the app never shows up in System Settings → Privacy →
 * Microphone. Recording the mic from Rust via cpal triggers the OS-level TCC
 * prompt the first time and registers the app properly.
 *
 * Lifecycle:
 * - keydown(Space) when no editable element has focus → invoke
 *   `audio.micCaptureStart(sessionId, lang)`. Backend opens the default input
 *   device and starts streaming PCM into a session buffer; every ~900ms it
 *   runs whisper on the cumulative buffer and emits `transcribe:partial`.
 * - keyup(Space) → invoke `audio.micCaptureStop(sessionId)` which halts the
 *   cpal stream, runs a final whisper pass, and returns the final transcript.
 */
export function usePushToTalk(opts: Options) {
  const { lang, onPartial, onFinalized, onError } = opts;
  const [state, setStateRaw] = useState<State>("idle");
  const stateRef = useRef<State>("idle");
  const setState = useCallback((s: State) => {
    stateRef.current = s;
    setStateRaw(s);
  }, []);

  const sessionIdRef = useRef<string | null>(null);
  const heldRef = useRef(false);

  // Stable opts ref so the keydown listener doesn't tear down on every render.
  const optsRef = useRef(opts);
  optsRef.current = opts;

  const startRecording = useCallback(async () => {
    if (stateRef.current !== "idle") return;
    if (optsRef.current.disabled) return;
    const sessionId = crypto.randomUUID();
    sessionIdRef.current = sessionId;
    // Optimistic state flip — `audio.micCaptureStart` blocks on whisper-model
    // download (~1-2 min) the first time. Without this, the UI looks frozen.
    setState("recording");
    try {
      await audio.micCaptureStart(sessionId, lang);
    } catch (e) {
      sessionIdRef.current = null;
      setState("idle");
      onError?.(String(e));
    }
  }, [lang, onError, setState]);

  const stopRecording = useCallback(async () => {
    const sid = sessionIdRef.current;
    sessionIdRef.current = null;
    if (!sid) {
      setState("idle");
      return;
    }
    setState("transcribing");
    try {
      const text = await audio.micCaptureStop(sid);
      if (text.trim()) onFinalized(text.trim());
    } catch (e) {
      onError?.(String(e));
    } finally {
      setState("idle");
    }
  }, [onFinalized, onError, setState]);

  // Bind keydown/keyup at the document level. Skip when an editable element
  // currently has focus (so typing Space in the textarea works normally).
  useEffect(() => {
    const isEditableTarget = (el: EventTarget | null): boolean => {
      if (!(el instanceof HTMLElement)) return false;
      const tag = el.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
      if (el.isContentEditable) return true;
      return false;
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.code !== "Space") return;
      if (e.repeat) return;
      if (heldRef.current) return;
      if (isEditableTarget(e.target)) return;
      if (optsRef.current.disabled) return;
      e.preventDefault();
      heldRef.current = true;
      void startRecording();
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.code !== "Space") return;
      if (!heldRef.current) return;
      heldRef.current = false;
      void stopRecording();
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, [startRecording, stopRecording]);

  // Subscribe to backend partial events for our session_id only.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    audio
      .onTranscribePartial((evt) => {
        if (evt.session_id === sessionIdRef.current) {
          onPartial?.(evt.text);
        }
      })
      .then((u) => {
        unlisten = u;
      });
    return () => unlisten?.();
  }, [onPartial]);

  // If the component unmounts mid-recording, abort cleanly.
  useEffect(() => {
    return () => {
      const sid = sessionIdRef.current;
      sessionIdRef.current = null;
      if (sid) {
        audio.micCaptureCancel(sid).catch(() => {});
      }
    };
  }, []);

  return { state };
}
