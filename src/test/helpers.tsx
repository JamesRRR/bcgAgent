import { type ReactNode } from "react";
import { ToasterProvider } from "@/components/Toaster";
import { AppContext, type AppCtx, type Page } from "@/state";
import { vi } from "vitest";
import "@/i18n";

export function makeAppCtx(overrides: Partial<AppCtx> = {}): AppCtx {
  return {
    page: "library" as Page,
    selectedGameId: null,
    setPage: vi.fn(),
    ...overrides,
  };
}

export function Wrapper({
  ctx,
  children,
}: {
  ctx?: AppCtx;
  children: ReactNode;
}) {
  return (
    <AppContext.Provider value={ctx ?? makeAppCtx()}>
      <ToasterProvider>{children}</ToasterProvider>
    </AppContext.Provider>
  );
}
