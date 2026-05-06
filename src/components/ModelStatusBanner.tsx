import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { inTauri } from "@/lib/transport";

type Status = {
  phase: "downloading" | "ready" | "error";
  bytes: number;
  total: number;
  message: string | null;
};

function formatGB(bytes: number): string {
  return (bytes / (1024 * 1024 * 1024)).toFixed(2);
}

export default function ModelStatusBanner() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<Status | null>(null);

  useEffect(() => {
    if (!inTauri) return;
    let unlisten: UnlistenFn | undefined;
    listen<Status>("app:model_status", (e) => {
      setStatus(e.payload);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  if (!status || status.phase === "ready") return null;

  const isError = status.phase === "error";
  const pct =
    status.total > 0
      ? Math.min(99, Math.round((status.bytes / status.total) * 100))
      : 0;

  return (
    <div
      role="status"
      aria-live="polite"
      className={
        "w-full px-4 py-2 text-sm flex items-center gap-3 " +
        (isError
          ? "bg-rose-50 text-rose-900 border-b border-rose-200"
          : "bg-amber-50 text-amber-900 border-b border-amber-200")
      }
    >
      <div className="flex-1 flex items-center gap-3">
        {!isError && (
          <span
            className="inline-block w-3 h-3 rounded-full bg-amber-500 animate-pulse"
            aria-hidden="true"
          />
        )}
        <span className="font-medium">
          {isError
            ? t("model.error")
            : t("model.preparing", {
                bytes: formatGB(status.bytes),
                total: formatGB(status.total),
              })}
        </span>
        {!isError && status.total > 0 && (
          <div className="hidden sm:block flex-1 max-w-xs h-1.5 bg-amber-200 rounded-full overflow-hidden">
            <div
              className="h-full bg-amber-600 transition-[width] duration-1000"
              style={{ width: `${pct}%` }}
            />
          </div>
        )}
      </div>
      {isError && status.message && (
        <span className="text-xs text-rose-700/80 truncate max-w-md">
          {status.message}
        </span>
      )}
    </div>
  );
}
