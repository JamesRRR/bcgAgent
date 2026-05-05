import { ArrowUpRight } from "lucide-react";
import type { RetrievedChunk } from "@/lib/ipc";

type Props = {
  chunk: RetrievedChunk;
  onOpen: (gameId: string) => void;
};

export default function CitationChip({ chunk, onOpen }: Props) {
  return (
    <button
      type="button"
      onClick={() => onOpen(chunk.game_id)}
      className="inline-flex items-center gap-1 rounded-full border border-ink/15 bg-cream px-3 py-1 text-xs text-ink hover:bg-paper hover:border-accent/40 transition-colors"
    >
      <span>
        《{chunk.game_name}》 p.{chunk.page_number}
      </span>
      <ArrowUpRight className="w-3 h-3" />
    </button>
  );
}
