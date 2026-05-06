import { useCallback, useMemo, useState, type ReactNode } from "react";
import { AppContext, type Page } from "@/state";
import SidebarNav from "@/components/SidebarNav";
import { ToasterProvider } from "@/components/Toaster";
import IngestProvider from "@/components/IngestProvider";
import ModelStatusBanner from "@/components/ModelStatusBanner";

export default function AppShell({ children }: { children: ReactNode }) {
  const [page, setPageState] = useState<Page>("library");
  const [selectedGameId, setSelectedGameId] = useState<string | null>(null);

  const setPage = useCallback(
    (p: Page, gameId?: string | null) => {
      if (gameId !== undefined) setSelectedGameId(gameId);
      setPageState(p);
    },
    [],
  );

  const value = useMemo(
    () => ({ page, selectedGameId, setPage }),
    [page, selectedGameId, setPage],
  );

  return (
    <AppContext.Provider value={value}>
      <ToasterProvider>
        <IngestProvider>
          <div className="flex flex-col min-h-screen bg-paper text-ink font-zh">
            <ModelStatusBanner />
            <div className="flex flex-1 min-h-0">
              <SidebarNav />
              <main className="flex-1 min-w-0">{children}</main>
            </div>
          </div>
        </IngestProvider>
      </ToasterProvider>
    </AppContext.Provider>
  );
}
