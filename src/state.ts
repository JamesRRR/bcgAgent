import { createContext, useContext } from "react";

export type Page =
  | "library"
  | "import"
  | "handbook"
  | "walkthrough"
  | "ask"
  | "settings";

export type AppCtx = {
  page: Page;
  selectedGameId: string | null;
  setPage: (p: Page, gameId?: string | null) => void;
};

export const AppContext = createContext<AppCtx | null>(null);

export function useApp(): AppCtx {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error("useApp must be used inside <AppShell>");
  return ctx;
}
