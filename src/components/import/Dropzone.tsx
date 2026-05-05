import { useEffect, useState } from "react";
import { Upload } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  pickFiles,
  ensurePickerMounted,
  PICKER_READY_EVENT,
} from "@/lib/picker";
import { inTauri } from "@/lib/transport";

const ALLOWED = /\.(jpe?g|png|webp|heic|heif)$/i;

type Props = {
  disabled?: boolean;
  onPicked: (paths: string[]) => void;
};

export default function Dropzone({ disabled, onPicked }: Props) {
  const { t } = useTranslation();
  const [hover, setHover] = useState(false);

  // Browser-mode (Playwright) fallback: hidden <input>.
  useEffect(() => {
    if (inTauri) return;
    ensurePickerMounted();
    const handler = (ev: Event) => {
      const detail = (ev as CustomEvent).detail;
      if (Array.isArray(detail) && detail.length > 0) onPicked(detail);
    };
    window.addEventListener(PICKER_READY_EVENT, handler);
    return () => window.removeEventListener(PICKER_READY_EVENT, handler);
  }, [onPicked]);

  // Tauri-mode: real desktop drag-drop on the webview window.
  useEffect(() => {
    if (!inTauri) return;
    let unlisten: (() => void) | undefined;
    (async () => {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      unlisten = await getCurrentWebview().onDragDropEvent((ev) => {
        const p = ev.payload;
        if (p.type === "enter") setHover(true);
        else if (p.type === "leave") setHover(false);
        else if (p.type === "drop") {
          setHover(false);
          if (disabled) return;
          const images = p.paths.filter((path) => ALLOWED.test(path));
          if (images.length > 0) onPicked(images);
        }
      });
    })();
    return () => {
      if (unlisten) unlisten();
    };
  }, [onPicked, disabled]);

  const handlePick = async () => {
    if (disabled) return;
    const paths = await pickFiles();
    if (paths.length) onPicked(paths);
  };

  return (
    <button
      type="button"
      onClick={handlePick}
      disabled={disabled}
      data-testid="dropzone"
      className={
        "w-full h-48 rounded-lg flex flex-col items-center justify-center gap-3 transition-colors disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-accent border-2 border-dashed " +
        (hover
          ? "bg-accent/10 border-accent text-ink"
          : "bg-paper border-ink/20 text-ink/70 hover:border-accent/60 hover:text-ink")
      }
    >
      <Upload className="w-8 h-8" />
      <span className="text-base">{t("import.dropzone")}</span>
    </button>
  );
}
