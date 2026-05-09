import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Search, Send } from "lucide-react";
import { Button } from "@/components/ui/button";
import VoiceButton from "./VoiceButton";

type Props = {
  busy: boolean;
  value: string;
  onChange: (v: string) => void;
  /// Called with the trimmed text and whether the user explicitly requested
  /// a research pass (the magnifier button toggle). The parent should run
  /// `cmd_explicit_research` BEFORE the normal ask call when `forceResearch`
  /// is true.
  onSubmit: (text: string, forceResearch: boolean) => void;
};

export default function AskBar({ busy, value, onChange, onSubmit }: Props) {
  const { t } = useTranslation();
  // Local pending submit-after-transcribe text (so we can fire submit
  // exactly once after the parent state has been updated).
  const [autoSubmit, setAutoSubmit] = useState<string | null>(null);
  // Wave 4: when toggled, the next submit fires explicit research first.
  const [forceResearch, setForceResearch] = useState(false);

  useEffect(() => {
    if (autoSubmit !== null && value === autoSubmit) {
      const text = autoSubmit;
      const force = forceResearch;
      setAutoSubmit(null);
      setForceResearch(false);
      onSubmit(text, force);
    }
  }, [autoSubmit, value, onSubmit, forceResearch]);

  const handleTranscribed = (text: string) => {
    onChange(text);
    setAutoSubmit(text);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      const trimmed = value.trim();
      if (trimmed && !busy) {
        const force = forceResearch;
        setForceResearch(false);
        onSubmit(trimmed, force);
      }
    }
  };

  const submit = () => {
    const trimmed = value.trim();
    if (trimmed && !busy) {
      const force = forceResearch;
      setForceResearch(false);
      onSubmit(trimmed, force);
    }
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
          data-testid="ask-input"
          className="flex-1 h-12 rounded-md border border-ink/20 bg-paper px-4 text-ink focus:outline-none focus:ring-2 focus:ring-accent disabled:opacity-50"
        />
        <Button
          variant={forceResearch ? "default" : "outline"}
          size="lg"
          aria-label={t("ask.searchWeb")}
          aria-pressed={forceResearch}
          title={t("ask.searchWeb")}
          onClick={() => setForceResearch((v) => !v)}
          disabled={busy}
          data-testid="ask-search-web-btn"
          className="!px-3 shrink-0"
        >
          <Search className="w-4 h-4" />
        </Button>
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
