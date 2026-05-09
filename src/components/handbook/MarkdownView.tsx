import { Image as ImageIcon } from "lucide-react";
import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ComponentPropsWithoutRef, ReactNode } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { inTauri } from "@/lib/transport";

/** Map from illustration token (e.g. "ill:0") to on-disk crop path. */
export type IllustrationMap = Record<
  string,
  { image_path: string; label: string | null }
>;

type Props = {
  source: string;
  highlight?: string;
  /** When provided, markdown `![label](ill:N)` images render as the actual
   *  cropped illustration figures instead of placeholder badges. */
  illustrations?: IllustrationMap;
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

export default function MarkdownView({
  source,
  highlight,
  illustrations,
}: Props) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      // The default urlTransform sanitizes anything that isn't http(s)/data/
      // mailto/tel — which strips our `ill:N` anchors. Pass them through.
      urlTransform={(url) =>
        url.startsWith("ill:") ? url : defaultUrlTransform(url)
      }
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
        img: ({ alt, src }: ComponentPropsWithoutRef<"img">) => {
          const srcStr = typeof src === "string" ? src : "";
          // Token-anchored illustration → render the real cropped figure
          // when we have it, otherwise fall through to the placeholder.
          if (srcStr.startsWith("ill:") && illustrations) {
            const ill = illustrations[srcStr];
            if (ill && ill.image_path) {
              const url = inTauri ? convertFileSrc(ill.image_path) : "";
              const caption = ill.label ?? alt ?? "";
              // We have to render inline-eligible markup because remark may
              // have wrapped this image inside a <p>. A <span> with role
              // figure keeps semantics without nesting block-level elements.
              return (
                <span
                  role="figure"
                  className="block my-3 rounded-md overflow-hidden border border-ink/10 bg-paper"
                >
                  <img
                    src={url}
                    alt={alt ?? ill.label ?? "illustration"}
                    className="w-full h-auto block"
                    loading="lazy"
                    draggable={false}
                  />
                  {caption && (
                    <span className="block px-2 py-1 text-xs text-ink/60 bg-cream/40">
                      {caption}
                    </span>
                  )}
                </span>
              );
            }
          }
          return (
            <span className="inline-flex items-center gap-1 px-2 py-0.5 bg-cream rounded text-xs align-middle mx-0.5">
              <ImageIcon className="w-3 h-3" />
              {alt ?? ""}
            </span>
          );
        },
        hr: () => <hr className="my-6 border-ink/10" />,
      }}
    >
      {source}
    </ReactMarkdown>
  );
}
