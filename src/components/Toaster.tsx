import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

export type ToastKind = "info" | "success" | "error";

export type Toast = {
  id: number;
  kind: ToastKind;
  text: string;
};

type ToasterCtx = {
  push: (text: string, kind?: ToastKind) => void;
};

const Ctx = createContext<ToasterCtx | null>(null);

export function useToaster(): ToasterCtx {
  const c = useContext(Ctx);
  if (!c) throw new Error("useToaster must be inside <ToasterProvider>");
  return c;
}

const KIND_CLS: Record<ToastKind, string> = {
  info: "bg-paper text-ink border-ink/15",
  success: "bg-accent text-cream border-accent",
  error: "bg-red-500 text-cream border-red-500",
};

export function ToasterProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const idRef = useRef(0);

  const push = useCallback((text: string, kind: ToastKind = "info") => {
    const id = ++idRef.current;
    setToasts((cur) => [...cur, { id, kind, text }]);
    window.setTimeout(() => {
      setToasts((cur) => cur.filter((t) => t.id !== id));
    }, 3000);
  }, []);

  const value = useMemo<ToasterCtx>(() => ({ push }), [push]);

  return (
    <Ctx.Provider value={value}>
      {children}
      <div className="pointer-events-none fixed bottom-4 right-4 z-50 flex flex-col gap-2">
        {toasts.map((t) => (
          <div
            key={t.id}
            className={`pointer-events-auto rounded-md border px-3 py-2 text-sm shadow-sm ${KIND_CLS[t.kind]}`}
          >
            {t.text}
          </div>
        ))}
      </div>
    </Ctx.Provider>
  );
}
