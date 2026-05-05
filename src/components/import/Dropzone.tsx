import { Upload } from "lucide-react";
import { useTranslation } from "react-i18next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

type Props = {
  disabled?: boolean;
  onPicked: (paths: string[]) => void;
};

export default function Dropzone({ disabled, onPicked }: Props) {
  const { t } = useTranslation();

  const handlePick = async () => {
    if (disabled) return;
    const selected = await openDialog({
      multiple: true,
      filters: [
        { name: "image", extensions: ["jpg", "jpeg", "png", "webp", "heic"] },
      ],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    if (paths.length) onPicked(paths);
  };

  return (
    <button
      type="button"
      onClick={handlePick}
      disabled={disabled}
      className="w-full h-48 bg-paper border-2 border-dashed border-ink/20 rounded-lg flex flex-col items-center justify-center gap-3 text-ink/70 hover:border-accent/60 hover:text-ink transition-colors disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-accent"
    >
      <Upload className="w-8 h-8" />
      <span className="text-base">{t("import.dropzone")}</span>
    </button>
  );
}
