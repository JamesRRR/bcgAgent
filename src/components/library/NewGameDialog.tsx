import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";

type Props = {
  open: boolean;
  onClose: () => void;
  onConfirm: (
    name_zh: string,
    name_en: string | undefined,
    publisher: string | undefined,
  ) => Promise<void> | void;
};

export default function NewGameDialog({ open, onClose, onConfirm }: Props) {
  const { t } = useTranslation();
  const [nameZh, setNameZh] = useState("");
  const [nameEn, setNameEn] = useState("");
  const [publisher, setPublisher] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const firstInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setNameZh("");
      setNameEn("");
      setPublisher("");
      setSubmitting(false);
      // Focus first input after mount
      const id = window.setTimeout(() => firstInputRef.current?.focus(), 0);
      return () => window.clearTimeout(id);
    }
  }, [open]);

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
        publisher.trim() ? publisher.trim() : undefined,
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
      aria-labelledby="new-game-dialog-title"
    >
      <form
        onSubmit={handleSubmit}
        onClick={(e) => e.stopPropagation()}
        className="bg-paper rounded-lg p-6 w-[420px] shadow-lg"
      >
        <h2
          id="new-game-dialog-title"
          className="text-xl font-semibold mb-4 text-ink"
        >
          {t("library.addGameDialog.title")}
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
            placeholder={t("library.addGameDialog.nameZhPlaceholder")}
            className="w-full h-10 rounded-md border border-ink/20 bg-cream px-3 text-ink focus:outline-none focus:ring-2 focus:ring-accent"
            data-testid="new-game-name-zh"
            required
          />
        </label>

        <label className="block mb-3">
          <span className="block text-sm text-ink/70 mb-1">
            {t("library.addGameDialog.nameEnLabel")}
          </span>
          <input
            type="text"
            value={nameEn}
            onChange={(e) => setNameEn(e.target.value)}
            className="w-full h-10 rounded-md border border-ink/20 bg-cream px-3 text-ink focus:outline-none focus:ring-2 focus:ring-accent"
            data-testid="new-game-name-en"
          />
        </label>

        <label className="block mb-5">
          <span className="block text-sm text-ink/70 mb-1">
            {t("library.addGameDialog.publisherLabel")}
          </span>
          <input
            type="text"
            value={publisher}
            onChange={(e) => setPublisher(e.target.value)}
            className="w-full h-10 rounded-md border border-ink/20 bg-cream px-3 text-ink focus:outline-none focus:ring-2 focus:ring-accent"
            data-testid="new-game-publisher"
          />
        </label>

        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" onClick={onClose}>
            {t("library.addGameDialog.cancel")}
          </Button>
          <Button type="submit" disabled={!canSubmit}>
            {t("library.addGameDialog.confirm")}
          </Button>
        </div>
      </form>
    </div>
  );
}
