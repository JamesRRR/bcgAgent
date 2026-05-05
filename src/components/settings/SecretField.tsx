import { useState } from "react";
import { Eye, EyeOff } from "lucide-react";

type Props = {
  label: string;
  value: string;
  saved: boolean;
  dirty: boolean;
  onChange: (v: string) => void;
  helperText: string;
  savedLabel: string;
  unsavedLabel: string;
};

export default function SecretField({
  label,
  value,
  saved,
  dirty,
  onChange,
  helperText,
  savedLabel,
  unsavedLabel,
}: Props) {
  const [visible, setVisible] = useState(false);

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between">
        <label className="text-sm font-medium text-ink/80 dark:text-cream/80">
          {label}
        </label>
        {(saved || dirty) && (
          <span
            className={`text-xs ${
              dirty ? "text-accent" : "text-ink/50 dark:text-cream/50"
            }`}
          >
            {dirty ? unsavedLabel : savedLabel}
          </span>
        )}
      </div>
      <div className="relative">
        <input
          type={visible ? "text" : "password"}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="w-full rounded-md border border-ink/15 bg-paper px-3 py-2 pr-10 text-sm text-ink outline-none focus:border-accent dark:bg-[var(--paper)] dark:text-cream dark:border-cream/15"
          autoComplete="off"
          spellCheck={false}
        />
        <button
          type="button"
          onClick={() => setVisible((v) => !v)}
          className="absolute right-2 top-1/2 -translate-y-1/2 text-ink/50 hover:text-ink dark:text-cream/50 dark:hover:text-cream"
          aria-label={visible ? "Hide" : "Show"}
        >
          {visible ? (
            <EyeOff className="h-4 w-4" />
          ) : (
            <Eye className="h-4 w-4" />
          )}
        </button>
      </div>
      <p className="text-xs text-ink/50 dark:text-cream/50">{helperText}</p>
    </div>
  );
}
