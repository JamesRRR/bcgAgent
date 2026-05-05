import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Send } from "lucide-react";
import { Button } from "@/components/ui/button";
import VoiceButton from "./VoiceButton";

type Props = {
  busy: boolean;
  value: string;
  onChange: (v: string) => void;
  onSubmit: (text: string) => void;
};

export default function AskBar({ busy, value, onChange, onSubmit }: Props) {
  const { t } = useTranslation();
  // Local pending submit-after-transcribe text (so we can fire submit
  // exactly once after the parent state has been updated).
  const [autoSubmit, setAutoSubmit] = useState<string | null>(null);

  useEffect(() => {
    if (autoSubmit !== null && value === autoSubmit) {
      const text = autoSubmit;
      setAutoSubmit(null);
      onSubmit(text);
    }
  }, [autoSubmit, value, onSubmit]);

  const handleTranscribed = (text: string) => {
    onChange(text);
    setAutoSubmit(text);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      const trimmed = value.trim();
      if (trimmed && !busy) onSubmit(trimmed);
    }
  };

  const submit = () => {
    const trimmed = value.trim();
    if (trimmed && !busy) onSubmit(trimmed);
  };

  return (
    <div className="flex items-end gap-3">
      <VoiceButton disabled={busy} onTranscribed={handleTranscribed} />
      <div className="flex-1 flex gap-2">
        <input
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={t("ask.placeholder")}
          disabled={busy}
          className="flex-1 h-12 rounded-md border border-ink/20 bg-paper px-4 text-ink focus:outline-none focus:ring-2 focus:ring-accent disabled:opacity-50"
        />
        <Button
          size="lg"
          onClick={submit}
          disabled={busy || !value.trim()}
          className="gap-2"
        >
          <Send className="w-4 h-4" />
          {t("ask.sendButton")}
        </Button>
      </div>
    </div>
  );
}
