import { useEffect } from "react";
import { Upload } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  pickFiles,
  ensurePickerMounted,
  PICKER_READY_EVENT,
} from "@/lib/picker";
import { inTauri } from "@/lib/transport";

type Props = {
  disabled?: boolean;
  onPicked: (paths: string[]) => void;
};

export default function Dropzone({ disabled, onPicked }: Props) {
  const { t } = useTranslation();

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

  const handlePick = async () => {
    if (disabled) return;
    const paths = await pickFiles();
    // In Tauri, `pickFiles` returns paths directly. In browser mode it returns
    // [] and the actual paths arrive via PICKER_READY_EVENT (handled above).
    if (paths.length) onPicked(paths);
  };

  return (
    <button
      type="button"
      onClick={handlePick}
      disabled={disabled}
      data-testid="dropzone"
      className="w-full h-48 bg-paper border-2 border-dashed border-ink/20 rounded-lg flex flex-col items-center justify-center gap-3 text-ink/70 hover:border-accent/60 hover:text-ink transition-colors disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-accent"
    >
      <Upload className="w-8 h-8" />
      <span className="text-base">{t("import.dropzone")}</span>
    </button>
  );
}
