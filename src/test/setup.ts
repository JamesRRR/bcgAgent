import "@testing-library/jest-dom/vitest";
import { afterEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";

afterEach(() => {
  cleanup();
});

// jsdom doesn't implement these — pages use them indirectly.
if (!window.matchMedia) {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: (q: string) => ({
      matches: false,
      media: q,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }),
  });
}

if (!("IntersectionObserver" in window)) {
  // @ts-expect-error stub
  window.IntersectionObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
    takeRecords() {
      return [];
    }
    root = null;
    rootMargin = "";
    thresholds = [];
  };
}

// Stub URL.createObjectURL used by some Tauri / framer-motion paths.
if (!URL.createObjectURL) {
  URL.createObjectURL = () => "blob:stub";
}
if (!URL.revokeObjectURL) {
  URL.revokeObjectURL = () => {};
}

// scrollIntoView is missing in jsdom.
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = vi.fn();
}

// jsdom 29 + Vitest 2 has a known incompatibility where localStorage.getItem
// is undefined when launched via vitest's CLI. Replace with an in-memory shim.
if (typeof localStorage === "undefined" || typeof localStorage.getItem !== "function") {
  const store = new Map<string, string>();
  const shim: Storage = {
    get length() {
      return store.size;
    },
    clear() {
      store.clear();
    },
    getItem(key) {
      return store.has(key) ? store.get(key)! : null;
    },
    key(idx) {
      return Array.from(store.keys())[idx] ?? null;
    },
    removeItem(key) {
      store.delete(key);
    },
    setItem(key, value) {
      store.set(key, String(value));
    },
  };
  Object.defineProperty(window, "localStorage", {
    configurable: true,
    value: shim,
  });
  Object.defineProperty(window, "sessionStorage", {
    configurable: true,
    value: { ...shim, ...{ length: 0 } },
  });
}
