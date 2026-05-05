import { Image as ImageIcon } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ComponentPropsWithoutRef, ReactNode } from "react";

type Props = {
  source: string;
  highlight?: string;
};

function highlightText(text: string, query: string): ReactNode {
  const q = query.trim();
  if (!q) return text;
  // case-insensitive split keeping the matches
  const escaped = q.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const re = new RegExp(`(${escaped})`, "gi");
  const parts = text.split(re);
  return parts.map((part, i) =>
    re.test(part) && part.toLowerCase() === q.toLowerCase() ? (
      <mark key={i} className="bg-accent/30 text-ink rounded px-0.5">
        {part}
      </mark>
    ) : (
      <span key={i}>{part}</span>
    ),
  );
}

// Wrap any string children with highlight spans.
function withHighlight(
  children: ReactNode,
  highlight: string | undefined,
): ReactNode {
  if (!highlight || !highlight.trim()) return children;
  if (typeof children === "string") return highlightText(children, highlight);
  if (Array.isArray(children)) {
    return children.map((c, i) =>
      typeof c === "string" ? (
        <span key={i}>{highlightText(c, highlight)}</span>
      ) : (
        c
      ),
    );
  }
  return children;
}

export default function MarkdownView({ source, highlight }: Props) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        h1: ({ children }: ComponentPropsWithoutRef<"h1">) => (
          <h1 className="text-3xl font-bold mt-8 mb-3 text-ink">
            {withHighlight(children, highlight)}
          </h1>
        ),
        h2: ({ children }: ComponentPropsWithoutRef<"h2">) => (
          <h2 className="text-2xl font-semibold mt-6 mb-2 text-ink">
            {withHighlight(children, highlight)}
          </h2>
        ),
        h3: ({ children }: ComponentPropsWithoutRef<"h3">) => (
          <h3 className="text-xl font-semibold mt-4 mb-2 text-ink">
            {withHighlight(children, highlight)}
          </h3>
        ),
        h4: ({ children }: ComponentPropsWithoutRef<"h4">) => (
          <h4 className="text-lg font-semibold mt-3 mb-2 text-ink">
            {withHighlight(children, highlight)}
          </h4>
        ),
        p: ({ children }: ComponentPropsWithoutRef<"p">) => (
          <p className="leading-7 my-3">{withHighlight(children, highlight)}</p>
        ),
        ul: ({ children }: ComponentPropsWithoutRef<"ul">) => (
          <ul className="my-3 pl-6 list-disc space-y-1">{children}</ul>
        ),
        ol: ({ children }: ComponentPropsWithoutRef<"ol">) => (
          <ol className="my-3 pl-6 list-decimal space-y-1">{children}</ol>
        ),
        li: ({ children }: ComponentPropsWithoutRef<"li">) => (
          <li className="leading-7">{withHighlight(children, highlight)}</li>
        ),
        table: ({ children }: ComponentPropsWithoutRef<"table">) => (
          <div className="my-4 overflow-x-auto">
            <table className="w-full border-collapse text-sm">{children}</table>
          </div>
        ),
        th: ({ children }: ComponentPropsWithoutRef<"th">) => (
          <th className="bg-cream text-left p-2 border border-ink/10 font-semibold">
            {withHighlight(children, highlight)}
          </th>
        ),
        td: ({ children }: ComponentPropsWithoutRef<"td">) => (
          <td className="p-2 border border-ink/10 align-top">
            {withHighlight(children, highlight)}
          </td>
        ),
        code: ({ children }: ComponentPropsWithoutRef<"code">) => (
          <code className="bg-cream px-1.5 py-0.5 rounded text-sm font-mono">
            {children}
          </code>
        ),
        blockquote: ({ children }: ComponentPropsWithoutRef<"blockquote">) => (
          <blockquote className="my-3 pl-4 border-l-4 border-accent/40 text-ink/80 italic">
            {children}
          </blockquote>
        ),
        a: ({ children, href }: ComponentPropsWithoutRef<"a">) => (
          <a
            href={href}
            className="text-accent underline underline-offset-2 hover:opacity-80"
            target="_blank"
            rel="noreferrer"
          >
            {withHighlight(children, highlight)}
          </a>
        ),
        img: ({ alt }: ComponentPropsWithoutRef<"img">) => (
          <span className="inline-flex items-center gap-1 px-2 py-0.5 bg-cream rounded text-xs align-middle mx-0.5">
            <ImageIcon className="w-3 h-3" />
            {alt ?? ""}
          </span>
        ),
        hr: () => <hr className="my-6 border-ink/10" />,
      }}
    >
      {source}
    </ReactMarkdown>
  );
}
