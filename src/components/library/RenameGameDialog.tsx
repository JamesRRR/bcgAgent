import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";

type Props = {
  open: boolean;
  initialNameZh: string;
  initialNameEn: string | null;
  onClose: () => void;
  onConfirm: (
    name_zh: string,
    name_en: string | undefined,
  ) => Promise<void> | void;
};

export default function RenameGameDialog({
  open,
  initialNameZh,
  initialNameEn,
  onClose,
  onConfirm,
}: Props) {
  const { t } = useTranslation();
  const [nameZh, setNameZh] = useState(initialNameZh);
  const [nameEn, setNameEn] = useState(initialNameEn ?? "");
  const [submitting, setSubmitting] = useState(false);
  const firstInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setNameZh(initialNameZh);
      setNameEn(initialNameEn ?? "");
      setSubmitting(false);
      // Wait for React to apply the new `value` before selecting, otherwise
      // select() runs on the previous (often empty) DOM value and a paste
      // appends instead of replacing.
      const id = window.requestAnimationFrame(() => {
        firstInputRef.current?.focus();
        firstInputRef.current?.select();
      });
      return () => window.cancelAnimationFrame(id);
    }
  }, [open, initialNameZh, initialNameEn]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  const trimmedZh = nameZh.trim();
  const canSubmit = trimmedZh.length > 0 && !submitting;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;
    setSubmitting(true);
    try {
      await onConfirm(
        trimmedZh,
        nameEn.trim() ? nameEn.trim() : undefined,
      );
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      className="fixed inset-0 bg-ink/40 flex items-center justify-center z-40"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-labelledby="rename-game-dialog-title"
    >
      <form
        onSubmit={handleSubmit}
        onClick={(e) => e.stopPropagation()}
        className="bg-paper rounded-lg p-6 w-[420px] shadow-lg"
      >
        <h2
          id="rename-game-dialog-title"
          className="text-xl font-semibold mb-4 text-ink"
        >
          {t("library.renameDialog.title")}
        </h2>

        <label className="block mb-3">
          <span className="block text-sm text-ink/70 mb-1">
            {t("library.addGameDialog.nameZhLabel")}
          </span>
          <input
            ref={firstInputRef}
            type="text"
            value={nameZh}
            onChange={(e) => setNameZh(e.target.value)}
            className="w-full h-10 rounded-md border border-ink/20 bg-cream px-3 text-ink focus:outline-none focus:ring-2 focus:ring-accent"
            data-testid="rename-game-name-zh"
            required
          />
        </label>

        <label className="block mb-5">
          <span className="block text-sm text-ink/70 mb-1">
            {t("library.addGameDialog.nameEnLabel")}
          </span>
          <input
            type="text"
            value={nameEn}
            onChange={(e) => setNameEn(e.target.value)}
            className="w-full h-10 rounded-md border border-ink/20 bg-cream px-3 text-ink focus:outline-none focus:ring-2 focus:ring-accent"
            data-testid="rename-game-name-en"
          />
        </label>

        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" onClick={onClose}>
            {t("library.addGameDialog.cancel")}
          </Button>
          <Button type="submit" disabled={!canSubmit}>
            {t("library.renameDialog.confirm")}
          </Button>
        </div>
      </form>
    </div>
  );
}
