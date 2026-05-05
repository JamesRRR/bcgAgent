// File-picker abstraction.
//
// In the Tauri shell we use the native plugin-dialog. In a regular browser
// (Playwright E2E) we use a hidden <input type="file"> — files are POSTed to
// `/api/upload_image` and the server returns absolute paths the Rust core
// can read. Both manual clicks and Playwright's setInputFiles drive the same
// path: a single permanent change handler dispatches a `bcg-picker-ready`
// CustomEvent with the resolved paths.

import { inTauri, HTTP_BASE_URL } from "@/lib/transport";

export const FILE_PICKER_INPUT_ID = "bcg-hidden-file-picker";
export const PICKER_READY_EVENT = "bcg-picker-ready";

async function uploadOne(file: File): Promise<string> {
  const fd = new FormData();
  fd.append("file", file, file.name);
  const res = await fetch(`${HTTP_BASE_URL}/api/upload_image`, {
    method: "POST",
    body: fd,
  });
  if (!res.ok) throw new Error(await res.text());
  const j = (await res.json()) as { path: string };
  return j.path;
}

function mountInput(): HTMLInputElement {
  const existing = document.getElementById(
    FILE_PICKER_INPUT_ID,
  ) as HTMLInputElement | null;
  if (existing) return existing;
  const el = document.createElement("input");
  el.type = "file";
  el.id = FILE_PICKER_INPUT_ID;
  el.multiple = true;
  el.accept = "image/*";
  el.setAttribute("data-testid", FILE_PICKER_INPUT_ID);
  el.style.position = "fixed";
  el.style.left = "-10000px";
  el.style.top = "0";
  el.style.width = "1px";
  el.style.height = "1px";
  el.style.opacity = "0";

  el.addEventListener("change", async () => {
    const files = el.files ? Array.from(el.files) : [];
    if (files.length === 0) {
      window.dispatchEvent(
        new CustomEvent(PICKER_READY_EVENT, { detail: [] }),
      );
      return;
    }
    try {
      const paths = await Promise.all(files.map(uploadOne));
      el.value = "";
      window.dispatchEvent(
        new CustomEvent(PICKER_READY_EVENT, { detail: paths }),
      );
    } catch (e) {
      window.dispatchEvent(
        new CustomEvent(PICKER_READY_EVENT, {
          detail: { error: String(e) },
        }),
      );
    }
  });

  document.body.appendChild(el);
  return el;
}

/// In Tauri: opens the native dialog and resolves with absolute paths.
/// In browser: clicks the hidden input. The Dropzone subscribes to
/// `PICKER_READY_EVENT` and receives paths there — this function resolves
/// with [] in browser mode (so callers shouldn't rely on its return value
/// in browser mode; subscribe to the global event instead).
export async function pickFiles(): Promise<string[]> {
  if (inTauri) {
    const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
    const selected = await openDialog({
      multiple: true,
      filters: [
        { name: "image", extensions: ["jpg", "jpeg", "png", "webp", "heic"] },
      ],
    });
    if (!selected) return [];
    return Array.isArray(selected) ? selected : [selected];
  }
  mountInput().click();
  return [];
}

/// Pre-mount the hidden picker so Playwright's `setInputFiles` can target it
/// before any user click. In Tauri this is a no-op.
export function ensurePickerMounted(): void {
  if (!inTauri && typeof document !== "undefined") {
    mountInput();
  }
}
