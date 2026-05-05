import { motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { Game } from "@/lib/ipc";

type Props = {
  game: Game;
  onClick: () => void;
};

export default function GameCard({ game, onClick }: Props) {
  const { t, i18n } = useTranslation();
  const firstChar = game.name_zh.charAt(0) || "?";
  const pageLabel =
    i18n.language.startsWith("zh")
      ? `${game.page_count} 页`
      : `${game.page_count} pages`;

  return (
    <motion.button
      type="button"
      onClick={onClick}
      whileHover={{ rotate: -1.5, y: -4 }}
      whileTap={{ scale: 0.98 }}
      transition={{ type: "spring", stiffness: 300, damping: 20 }}
      className="group flex flex-col text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-accent rounded-md"
      aria-label={game.name_zh}
    >
      <div className="relative w-full aspect-[3/4] rounded-md border border-ink/10 bg-paper overflow-hidden shadow-sm">
        {game.cover_path ? (
          <img
            src={convertFileSrc(game.cover_path)}
            alt={game.name_zh}
            className="w-full h-full object-cover"
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center bg-paper relative">
            <span
              className="font-handwritten text-accent leading-none"
              style={{ fontSize: "8rem" }}
            >
              {firstChar}
            </span>
            <svg
              viewBox="0 0 24 24"
              className="absolute bottom-3 right-3 w-7 h-7 text-ink/20"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              aria-hidden="true"
            >
              <rect x="3" y="3" width="18" height="18" rx="3" />
              <circle cx="8" cy="8" r="1.2" fill="currentColor" />
              <circle cx="16" cy="8" r="1.2" fill="currentColor" />
              <circle cx="12" cy="12" r="1.2" fill="currentColor" />
              <circle cx="8" cy="16" r="1.2" fill="currentColor" />
              <circle cx="16" cy="16" r="1.2" fill="currentColor" />
            </svg>
          </div>
        )}
      </div>
      <div className="mt-3 px-1">
        <h3 className="font-zh font-semibold text-ink leading-tight">
          {game.name_zh}
        </h3>
        {game.name_en && (
          <p className="text-sm text-ink/60 mt-0.5">{game.name_en}</p>
        )}
        <p className="text-xs text-ink/50 mt-1" aria-label={t("library.title")}>
          {pageLabel}
        </p>
      </div>
    </motion.button>
  );
}
