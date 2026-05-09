import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowUpRight,
  BookOpen,
  Globe,
  MessagesSquare,
  Palette,
  ThumbsDown,
  ThumbsUp,
} from "lucide-react";
import { research, type RetrievedChunk, type TrustTier } from "@/lib/ipc";

type Props = {
  chunk: RetrievedChunk;
  onOpen: (gameId: string) => void;
};

const TIER_ORDER: TrustTier[] = ["publisher", "designer", "community", "unverified"];

function tierFor(chunk: RetrievedChunk): TrustTier {
  const t = chunk.trust_tier;
  return t && TIER_ORDER.includes(t) ? t : "publisher";
}

function TierBadge({ tier }: { tier: TrustTier }) {
  const cls =
    "inline-flex h-4 w-4 items-center justify-center text-ink/70";
  switch (tier) {
    case "publisher":
      return <BookOpen className={cls} aria-label="publisher" />;
    case "designer":
      return <Palette className={cls} aria-label="designer" />;
    case "community":
      return <MessagesSquare className={cls} aria-label="community" />;
    case "unverified":
    default:
      return <Globe className={cls} aria-label="unverified" />;
  }
}

export default function CitationChip({ chunk, onOpen }: Props) {
  const { t } = useTranslation();
  const tier = tierFor(chunk);
  const [endorsed, setEndorsed] = useState<boolean | null>(
    chunk.endorsed ?? null,
  );

  const tooltip =
    chunk.source_url
      ? `${chunk.source_url}`
      : `《${chunk.game_name}》 p.${chunk.page_number}`;

  const handleOpen = () => {
    // Community / unverified chips open the source URL when present;
    // otherwise fall through to the page reader.
    if (
      (tier === "community" || tier === "unverified") &&
      chunk.source_url
    ) {
      try {
        window.open(chunk.source_url, "_blank", "noopener,noreferrer");
        return;
      } catch {
        /* fall through */
      }
    }
    onOpen(chunk.game_id);
  };

  const setEndorsement = async (up: boolean) => {
    // Optimistic update — revert on error.
    const prev = endorsed;
    setEndorsed(up);
    try {
      await research.endorseChunk(chunk.chunk_id, up);
    } catch {
      setEndorsed(prev);
    }
  };

  return (
    <span className="group relative inline-flex items-center">
      <button
        type="button"
        onClick={handleOpen}
        title={tooltip}
        data-tier={tier}
        data-testid={`citation-chip-${tier}`}
        className="inline-flex items-center gap-1.5 rounded-full border border-ink/15 bg-cream px-3 py-1 text-xs text-ink hover:bg-paper hover:border-accent/40 transition-colors"
      >
        <TierBadge tier={tier} />
        <span>
          《{chunk.game_name}》 p.{chunk.page_number}
        </span>
        <ArrowUpRight className="w-3 h-3" />
      </button>
      {/* Thumbs always rendered; faded until hover so the chip stays compact. */}
      <span className="ml-1 inline-flex items-center gap-0.5 opacity-30 group-hover:opacity-100 transition-opacity">
        <button
          type="button"
          aria-label={t("ask.citations.thumbsUp")}
          onClick={() => setEndorsement(true)}
          data-testid="citation-thumbs-up"
          className={`rounded p-1 hover:bg-cream ${
            endorsed === true ? "text-accent" : "text-ink/50"
          }`}
        >
          <ThumbsUp className="w-3 h-3" />
        </button>
        <button
          type="button"
          aria-label={t("ask.citations.thumbsDown")}
          onClick={() => setEndorsement(false)}
          data-testid="citation-thumbs-down"
          className={`rounded p-1 hover:bg-cream ${
            endorsed === false ? "text-accent" : "text-ink/50"
          }`}
        >
          <ThumbsDown className="w-3 h-3" />
        </button>
      </span>
    </span>
  );
}
