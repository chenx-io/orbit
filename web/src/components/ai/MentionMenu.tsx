// The `@` mention menu inside the input (absolutely positioned above the input).
//
// Popover/DropdownMenu are deliberately **not** used: focus must stay in the textarea,
// and keyboard navigation (↑↓/Enter/Esc) is handled by the Composer's onKeyDown while this component only renders.
import { Boxes, FileCode, FolderTree, ListChecks, Webhook } from "lucide-react";
import type { AiReferenceKind } from "@/data/aiTypes";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import type { ReferenceCandidate } from "@/lib/ai/references";

const KIND_ICONS: Record<AiReferenceKind, typeof Boxes> = {
  workspace: Boxes,
  collection: FolderTree,
  request: Webhook,
  scenario: ListChecks,
  file: FileCode,
};

export interface MentionMenuProps {
  items: ReferenceCandidate[];
  /** Currently highlighted index (-1 = none) */
  highlight: number;
  /** Fixed actions such as "Pick from file..." (always last) */
  footer?: { label: string; onPick: () => void };
  onHover: (index: number) => void;
  onPick: (item: ReferenceCandidate) => void;
}

export function MentionMenu({
  items,
  highlight,
  footer,
  onHover,
  onPick,
}: MentionMenuProps) {
  const { t } = useT();
  const empty = items.length === 0 && !footer;

  return (
    <div className="absolute bottom-full left-0 z-50 mb-1.5 w-full max-w-md overflow-hidden rounded-md border border-border bg-popover shadow-md">
      <div className="border-b border-border px-2.5 py-1.5 text-xs text-muted-foreground">
        {t("ai.reference.mentionTitle")}
      </div>
      <div className="max-h-64 overflow-y-auto py-0.5">
        {empty && (
          <p className="px-2.5 py-2 text-xs text-muted-foreground">
            {t("ai.reference.mentionEmpty")}
          </p>
        )}
        {items.map((item, index) => {
          const Icon = KIND_ICONS[item.kind];
          return (
            <button
              key={`${item.kind}:${item.id}`}
              type="button"
              // Prevent the textarea from losing focus on click (losing focus closes the menu and the click goes nowhere)
              onMouseDown={(e) => e.preventDefault()}
              onMouseEnter={() => onHover(index)}
              onClick={() => onPick(item)}
              className={cn(
                "flex w-full cursor-pointer items-center gap-2 px-2.5 py-1.5 text-left",
                index === highlight && "bg-accent",
              )}
            >
              <Icon className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              <span className="min-w-0 flex-1 truncate text-xs">
                {item.name}
              </span>
              {item.detail && (
                <span className="max-w-40 truncate font-mono text-xs text-muted-foreground">
                  {item.detail}
                </span>
              )}
            </button>
          );
        })}
        {footer && (
          <button
            type="button"
            onMouseDown={(e) => e.preventDefault()}
            onClick={footer.onPick}
            className={cn(
              "flex w-full cursor-pointer items-center gap-2 border-t border-border px-2.5 py-1.5 text-left text-xs hover:bg-accent",
              highlight === items.length && "bg-accent",
            )}
          >
            <FileCode className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
            {footer.label}
          </button>
        )}
      </div>
    </div>
  );
}
