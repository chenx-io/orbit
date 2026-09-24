// Markdown rendering for AI replies (GFM: tables / task lists / strikethrough).
//
// Security: react-markdown does **not parse raw HTML** by default (rehype-raw is not pulled in here either),
// so a `<script>` or `<img onerror=...>` in model output is shown as plain text and needs no extra sanitizing.
import { memo, useState } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { Check, Copy } from "lucide-react";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

/** Extract the plain text of a code block (for the copy button; the `code` element's children may be an array). */
function extractText(node: React.ReactNode): string {
  if (typeof node === "string") return node;
  if (Array.isArray(node)) return node.map(extractText).join("");
  if (node && typeof node === "object" && "props" in node) {
    return extractText(
      (node as { props?: { children?: React.ReactNode } }).props?.children,
    );
  }
  return "";
}

/** Code block: monospace + horizontally scrollable + a copy button (the JSON/scripts AI returns are usually ready to use). */
function CodeBlock({ code, lang }: { code: string; lang?: string }) {
  const { t } = useT();
  const [copied, setCopied] = useState(false);
  return (
    <div className="group relative my-1.5 overflow-hidden rounded-md border border-border bg-muted/40">
      <div className="flex items-center gap-2 border-b border-border px-2 py-1">
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">
          {lang || "text"}
        </span>
        <button
          type="button"
          title={t("common.copy")}
          className="flex shrink-0 cursor-pointer items-center gap-1 rounded px-1 text-xs text-muted-foreground hover:text-foreground"
          onClick={() => {
            void navigator.clipboard?.writeText(code).then(() => {
              setCopied(true);
              window.setTimeout(() => setCopied(false), 1200);
            });
          }}
        >
          {copied ? (
            <Check className="h-3 w-3 text-success" />
          ) : (
            <Copy className="h-3 w-3" />
          )}
          {copied ? t("common.copied") : t("common.copy")}
        </button>
      </div>
      <pre className="overflow-x-auto px-2 py-1.5 font-mono text-xs leading-relaxed">
        <code>{code}</code>
      </pre>
    </div>
  );
}

const COMPONENTS: Components = {
  p: ({ children }) => (
    <p className="my-1.5 break-words first:mt-0 last:mb-0">{children}</p>
  ),
  h1: ({ children }) => (
    <h3 className="mb-1 mt-2.5 text-sm font-semibold first:mt-0">{children}</h3>
  ),
  h2: ({ children }) => (
    <h4 className="mb-1 mt-2.5 text-sm font-semibold first:mt-0">{children}</h4>
  ),
  h3: ({ children }) => (
    <h5 className="mb-1 mt-2 text-xs font-semibold first:mt-0">{children}</h5>
  ),
  h4: ({ children }) => (
    <h6 className="mb-1 mt-2 text-xs font-semibold first:mt-0">{children}</h6>
  ),
  ul: ({ children }) => (
    <ul className="my-1.5 list-disc space-y-0.5 pl-4 first:mt-0 last:mb-0">
      {children}
    </ul>
  ),
  ol: ({ children }) => (
    <ol className="my-1.5 list-decimal space-y-0.5 pl-4 first:mt-0 last:mb-0">
      {children}
    </ol>
  ),
  li: ({ children }) => <li className="break-words [&>p]:my-0">{children}</li>,
  a: ({ children, href }) => (
    <a
      href={href}
      target="_blank"
      rel="noreferrer noopener"
      className="text-primary underline underline-offset-2"
    >
      {children}
    </a>
  ),
  blockquote: ({ children }) => (
    <blockquote className="my-1.5 border-l-2 border-border pl-2.5 text-muted-foreground">
      {children}
    </blockquote>
  ),
  hr: () => <hr className="my-2 border-border" />,
  // Inline code: short backticked fragments (code blocks take the pre branch and never reach here)
  code: ({ children, className }) =>
    className?.includes("language-") ? (
      <code className="font-mono">{children}</code>
    ) : (
      <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">
        {children}
      </code>
    ),
  pre: ({ children }) => {
    const child = children as
      | React.ReactElement<{ children?: React.ReactNode; className?: string }>
      | undefined;
    const lang = /language-([\w-]+)/.exec(child?.props?.className ?? "")?.[1];
    return <CodeBlock code={extractText(child?.props?.children)} lang={lang} />;
  },
  table: ({ children }) => (
    <div className="my-1.5 overflow-x-auto rounded-md border border-border">
      <table className="w-full border-collapse text-xs">{children}</table>
    </div>
  ),
  thead: ({ children }) => <thead className="bg-muted/50">{children}</thead>,
  th: ({ children }) => (
    <th className="border-b border-border px-2 py-1 text-left font-semibold">
      {children}
    </th>
  ),
  td: ({ children }) => (
    <td className="border-b border-border/60 px-2 py-1 align-top">
      {children}
    </td>
  ),
  input: ({ checked }) => (
    <input
      type="checkbox"
      checked={Boolean(checked)}
      readOnly
      className="mr-1 align-middle"
    />
  ),
};

export const Markdown = memo(function Markdown({
  text,
  className,
}: {
  text: string;
  className?: string;
}) {
  return (
    <div className={cn("break-words text-xs leading-relaxed", className)}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={COMPONENTS}>
        {text}
      </ReactMarkdown>
    </div>
  );
});
