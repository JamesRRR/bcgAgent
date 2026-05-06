import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "uptodate" }
  | { status: "available"; version: string; notes?: string; install: () => Promise<void> }
  | { status: "downloading"; version: string; downloaded: number; total?: number; pct: number }
  | { status: "ready"; version: string }
  | { status: "error"; message: string };

// Smooth, monotonic progress. The Tauri updater plugin's events have two
// gotchas we work around:
//   1. `Started.contentLength` can be 0 or undefined when the server doesn't
//      send Content-Length. Without a denominator we'd snap between 30%
//      (fallback) and a real percent — so we maintain a stable estimate.
//   2. We never let `pct` go down. macOS download is a single HTTP transfer,
//      so a decreasing percent is always a glitch (not a restart).
export async function checkForUpdate(
  onState: (s: UpdateState) => void
): Promise<void> {
  onState({ status: "checking" });
  try {
    const update = await check();
    if (!update) {
      onState({ status: "uptodate" });
      return;
    }
    onState({
      status: "available",
      version: update.version,
      notes: update.body,
      install: async () => {
        let downloaded = 0;
        let total: number | undefined;
        let lastPct = 0;
        const emit = () => {
          let pct: number;
          if (total && total > 0) {
            pct = Math.min(99, Math.floor((downloaded / total) * 100));
          } else {
            // Unknown total: log curve that asymptotes toward 95% so the bar
            // always advances but never finishes until "Finished" arrives.
            // ~30 MB worth of bytes ≈ 50% of the bar.
            const mb = downloaded / (1024 * 1024);
            pct = Math.floor(95 * (1 - Math.exp(-mb / 30)));
          }
          if (pct < lastPct) pct = lastPct;
          lastPct = pct;
          onState({
            status: "downloading",
            version: update.version,
            downloaded,
            total,
            pct,
          });
        };

        await update.downloadAndInstall((event) => {
          switch (event.event) {
            case "Started":
              downloaded = 0;
              total = event.data.contentLength ?? undefined;
              lastPct = 0;
              emit();
              break;
            case "Progress":
              downloaded += event.data.chunkLength ?? 0;
              emit();
              break;
            case "Finished":
              lastPct = 100;
              onState({ status: "ready", version: update.version });
              break;
          }
        });
        await relaunch();
      },
    });
  } catch (e) {
    onState({ status: "error", message: e instanceof Error ? e.message : String(e) });
  }
}
