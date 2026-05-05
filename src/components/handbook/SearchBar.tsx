import { Search } from "lucide-react";
import { useEffect, useState } from "react";

type Props = {
  value: string;
  onChange: (next: string) => void;
  placeholder: string;
};

export default function SearchBar({ value, onChange, placeholder }: Props) {
  const [local, setLocal] = useState(value);

  // Keep internal mirror in sync if parent resets.
  useEffect(() => {
    setLocal(value);
  }, [value]);

  // Debounce 250ms.
  useEffect(() => {
    if (local === value) return;
    const t = setTimeout(() => onChange(local), 250);
    return () => clearTimeout(t);
  }, [local, value, onChange]);

  return (
    <div className="relative w-64">
      <Search className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-ink/40 pointer-events-none" />
      <input
        type="text"
        value={local}
        onChange={(e) => setLocal(e.target.value)}
        placeholder={placeholder}
        className="w-full h-9 pl-9 pr-3 rounded-md bg-paper border border-ink/15 text-sm text-ink placeholder:text-ink/40 focus:outline-none focus:ring-2 focus:ring-accent"
      />
    </div>
  );
}
