import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useApp } from "@/state";
import type { RetrievedChunk } from "@/lib/ipc";
import CitationChip from "./CitationChip";

type Props = {
  question: string | null;
  answer: string;
  citations: RetrievedChunk[];
  streaming: boolean;
};

export default function AnswerCard({
  question,
  answer,
  citations,
  streaming,
}: Props) {
  const { t } = useTranslation();
  const { setPage } = useApp();
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: "end" });
  }, [answer, citations]);

  if (!question && !answer) {
    return (
      <div className="rounded-lg border border-ink/10 bg-paper p-8 text-center text-ink/50">
        {t("ask.placeholder")}
      </div>
    );
  }

  return (
    <div className="relative rounded-lg border border-ink/10 bg-paper p-6 overflow-hidden">
      {streaming && (
        <div className="absolute top-0 left-0 right-0 h-0.5 bg-accent/20 overflow-hidden">
          <div className="h-full w-1/3 bg-accent animate-[progress_1.5s_ease-in-out_infinite]" />
        </div>
      )}
      {question && (
        <p className="italic text-sm text-ink/70 mb-4">{question}</p>
      )}
      <div className="text-ink leading-7 whitespace-pre-wrap min-h-[1.5rem]">
        {answer}
        {streaming && <span className="inline-block w-1.5 h-4 ml-0.5 bg-accent/60 animate-pulse align-middle" />}
      </div>
      {citations.length > 0 && (
        <div className="mt-6 pt-4 border-t border-ink/10">
          <p className="text-xs uppercase tracking-wide text-ink/50 mb-2">
            {t("ask.citations.heading")}
          </p>
          <div className="flex flex-wrap gap-2">
            {citations.map((c) => (
              <CitationChip
                key={c.chunk_id}
                chunk={c}
                onOpen={(gameId) => setPage("handbook", gameId)}
              />
            ))}
          </div>
        </div>
      )}
      <div ref={bottomRef} />
    </div>
  );
}
