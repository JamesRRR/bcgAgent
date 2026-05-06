import { useEffect, useState } from "react";
import { checkForUpdate, type UpdateState } from "@/lib/updater";

export default function UpdaterBanner() {
  const [state, setState] = useState<UpdateState>({ status: "idle" });
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    const t = setTimeout(() => {
      checkForUpdate(setState);
    }, 3000);
    return () => clearTimeout(t);
  }, []);

  // Listen for a "user clicked check-for-updates in Settings" custom event so
  // we can re-trigger the check on demand without remounting the component.
  useEffect(() => {
    const handler = () => {
      setDismissed(false);
      checkForUpdate(setState);
    };
    window.addEventListener("bcg:check-for-update", handler);
    return () => window.removeEventListener("bcg:check-for-update", handler);
  }, []);

  if (dismissed) return null;
  if (
    state.status === "idle" ||
    state.status === "checking" ||
    state.status === "uptodate"
  ) {
    return null;
  }

  return (
    <div className="fixed bottom-4 right-4 z-50 max-w-sm rounded-lg border border-zinc-200 bg-white p-4 shadow-lg dark:border-zinc-800 dark:bg-zinc-900">
      {state.status === "available" && (
        <>
          <div className="text-sm font-medium">新版本可用 v{state.version}</div>
          {state.notes && (
            <div className="mt-1 max-h-24 overflow-auto text-xs text-zinc-600 dark:text-zinc-400">
              {state.notes}
            </div>
          )}
          <div className="mt-3 flex gap-2">
            <button
              onClick={() => state.install().catch(() => {})}
              className="rounded bg-zinc-900 px-3 py-1.5 text-xs font-medium text-white hover:bg-zinc-700 dark:bg-white dark:text-zinc-900"
            >
              立即更新
            </button>
            <button
              onClick={() => setDismissed(true)}
              className="rounded border border-zinc-300 px-3 py-1.5 text-xs font-medium hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
            >
              稍后
            </button>
          </div>
        </>
      )}
      {state.status === "downloading" && (
        <>
          <div className="text-sm font-medium">下载更新 v{state.version}</div>
          <div className="mt-2 h-1.5 w-full overflow-hidden rounded bg-zinc-200 dark:bg-zinc-800">
            <div
              className="h-full bg-zinc-900 transition-all dark:bg-white"
              style={{
                width: state.total
                  ? `${Math.min(100, (state.downloaded / state.total) * 100)}%`
                  : "30%",
              }}
            />
          </div>
        </>
      )}
      {state.status === "ready" && (
        <div className="text-sm font-medium">更新就绪 — 重启中…</div>
      )}
      {state.status === "error" && (
        <>
          <div className="text-sm font-medium text-rose-700">检查更新失败</div>
          <div className="mt-1 text-xs text-rose-600/90 break-words max-h-24 overflow-auto">
            {state.message}
          </div>
          <div className="mt-3 flex gap-2">
            <button
              onClick={() => checkForUpdate(setState)}
              className="rounded bg-zinc-900 px-3 py-1.5 text-xs font-medium text-white hover:bg-zinc-700 dark:bg-white dark:text-zinc-900"
            >
              重试
            </button>
            <button
              onClick={() => setDismissed(true)}
              className="rounded border border-zinc-300 px-3 py-1.5 text-xs font-medium hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
            >
              关闭
            </button>
          </div>
        </>
      )}
    </div>
  );
}
