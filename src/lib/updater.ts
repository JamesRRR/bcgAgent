import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "uptodate" }
  | { status: "available"; version: string; notes?: string; install: () => Promise<void> }
  | { status: "downloading"; version: string; downloaded: number; total?: number }
  | { status: "ready"; version: string }
  | { status: "error"; message: string };

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
        await update.downloadAndInstall((event) => {
          switch (event.event) {
            case "Started":
              total = event.data.contentLength;
              onState({ status: "downloading", version: update.version, downloaded: 0, total });
              break;
            case "Progress":
              downloaded += event.data.chunkLength;
              onState({ status: "downloading", version: update.version, downloaded, total });
              break;
            case "Finished":
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
