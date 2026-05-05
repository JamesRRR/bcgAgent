// Transport abstraction. In the Tauri shell we use the native invoke + listen
// APIs. Under a regular browser (the Playwright E2E suite) we hit the
// `bcgagent_lib::test_server` HTTP shim that exposes the same commands at
// `POST /api/<cmd>` and bridges events through a single SSE stream.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";

export const inTauri =
  typeof window !== "undefined" &&
  // The Tauri runtime injects this object before any user JS executes.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  ("__TAURI_INTERNALS__" in (window as any) ||
    "__TAURI__" in (window as any));

const HTTP_BASE =
  typeof window !== "undefined"
    ? (window.location.protocol === "https:"
        ? "https://"
        : "http://") + "localhost:1421"
    : "http://localhost:1421";

async function httpInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const res = await fetch(`${HTTP_BASE}/api/${cmd}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(args ?? {}),
  });
  if (!res.ok) {
    const txt = await res.text();
    throw new Error(txt || `${cmd} failed (${res.status})`);
  }
  // Some commands return `null` (Option<T>) as JSON literal `null`. Some return
  // unit which axum encodes as `null`. Both deserialize the same way here.
  const text = await res.text();
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}

export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (inTauri) return tauriInvoke<T>(cmd, args);
  return httpInvoke<T>(cmd, args);
}

// ------------------ events ------------------

type EventHandler<T> = (payload: T) => void;

interface SseHub {
  add<T>(name: string, cb: EventHandler<T>): UnlistenFn;
  ready: Promise<void>;
}

let hub: SseHub | null = null;

function buildHub(): SseHub {
  const handlers = new Map<string, Set<EventHandler<unknown>>>();
  const es = new EventSource(`${HTTP_BASE}/api/events`);

  const ready = new Promise<void>((resolve, reject) => {
    es.onopen = () => resolve();
    es.onerror = () => {
      // EventSource can fire `error` transiently while still streaming.
      // Resolve after a short delay so callers don't hang.
      setTimeout(resolve, 500);
      void reject;
    };
  });

  const subscribed = new Set<string>();
  const ensureSubscribed = (name: string) => {
    if (subscribed.has(name)) return;
    subscribed.add(name);
    es.addEventListener(name, (ev) => {
      let parsed: unknown;
      try {
        const wire = JSON.parse((ev as MessageEvent).data) as {
          kind: string;
          data: unknown;
        };
        parsed = wire.data;
      } catch {
        parsed = (ev as MessageEvent).data;
      }
      const set = handlers.get(name);
      if (!set) return;
      for (const h of set) h(parsed);
    });
  };

  return {
    ready,
    add(name, cb) {
      ensureSubscribed(name);
      let set = handlers.get(name);
      if (!set) {
        set = new Set();
        handlers.set(name, set);
      }
      set.add(cb as EventHandler<unknown>);
      return () => {
        set!.delete(cb as EventHandler<unknown>);
      };
    },
  };
}

function getHub(): SseHub {
  if (!hub) hub = buildHub();
  return hub;
}

export async function listen<T>(
  event: string,
  cb: (e: { payload: T }) => void,
): Promise<UnlistenFn> {
  if (inTauri) return tauriListen<T>(event, cb);
  const h = getHub();
  // Wait for the SSE connection to be open before returning. This prevents
  // the caller (e.g. `await ingest.onPageStarted(...)`) from immediately
  // firing an HTTP request whose events would race against the SSE connect.
  await h.ready;
  return h.add<T>(event, (payload) => cb({ payload }));
}

export const HTTP_BASE_URL = HTTP_BASE;
