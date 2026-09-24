// Change card: shows the changes an AI write **already persisted**, in a compact table.
//
// The old "one colored block per field plus a preformatted block for before and after" took far too much vertical space, while for create-type changes
// (the vast majority) the "before" side is always empty — extremely low information density. Now:
// - create: a two-column table (field | value), **dropping rows with no information** (empty values, defaults such as "auth.type=none");
// - update: a three-column table (field | before | after).
// The card is purely "after-the-fact audit", so it has no action buttons.
import { useMemo } from "react";
import type { AiFieldDiff, AiProposal } from "@/data/aiTypes";
import { useT } from "@/lib/i18n";
import {
  formatProposalValue,
  isNoiseDiff,
  pickImportantDiffs,
} from "@/lib/ai/proposal";

/** Maximum rows shown in the table (the rest collapse into one hint row). */
const MAX_ROWS = 20;

export interface ProposalCardProps {
  proposal: AiProposal;
}

export function ProposalCard({ proposal }: ProposalCardProps) {
  const { t } = useT();
  const isCreate = !proposal.before;

  const { rows, hidden } = useMemo(() => {
    const all = proposal.diffs ?? [];
    // On create the "before" side is always empty, so noise detection only targets the new value; on update deletions must be kept
    const meaningful = all.filter((d) => !isNoiseDiff(d, isCreate));
    const picked = pickImportantDiffs(meaningful, MAX_ROWS);
    return { rows: picked, hidden: meaningful.length - picked.length };
  }, [proposal.diffs, isCreate]);

  return (
    // Collapsed by default: the conversation only needs to know "what changed", with field-level detail on demand.
    // A permanent table stretches the conversation enormously and these details add no value in the conversation context (their audit value remains,
    // simply click to view). Kept in the same shape as "thinking" and "tool results".
    <details className="overflow-hidden rounded-md border border-border bg-background">
      <summary className="flex cursor-pointer items-center gap-2 bg-muted/40 px-2.5 py-1.5">
        <span className="min-w-0 flex-1 truncate text-xs font-semibold">
          {proposal.title}
        </span>
        <span className="shrink-0 text-xs text-success">
          {t("ai.proposal.applied")}
        </span>
      </summary>
      <div className="truncate border-t border-border px-2.5 py-1 text-xs text-muted-foreground">
        {proposal.target}
      </div>

      <div className="max-h-72 overflow-auto border-t border-border">
        <table className="w-full border-collapse text-xs">
          <thead className="bg-muted/40 text-muted-foreground">
            <tr>
              <th className="w-px whitespace-nowrap border-b border-border px-2 py-1 text-left font-medium">
                {t("ai.proposal.field")}
              </th>
              {!isCreate && (
                <th className="w-px whitespace-nowrap border-b border-border px-2 py-1 text-left font-medium">
                  {t("ai.proposal.before")}
                </th>
              )}
              <th className="border-b border-border px-2 py-1 text-left font-medium">
                {t("ai.proposal.after")}
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((d) => (
              <Row
                key={`${d.path}-${d.kind}`}
                diff={d}
                showBefore={!isCreate}
              />
            ))}
          </tbody>
        </table>
        {rows.length === 0 && (
          <p className="px-2.5 py-2 text-xs text-muted-foreground">
            {t("ai.proposal.noMeaningfulChange")}
          </p>
        )}
        {hidden > 0 && (
          <p className="px-2.5 py-1 text-xs text-muted-foreground">
            {t("ai.proposal.moreChanges")} +{hidden}
          </p>
        )}
      </div>
    </details>
  );
}

function Row({ diff, showBefore }: { diff: AiFieldDiff; showBefore: boolean }) {
  const after = formatProposalValue(diff.after);
  const before = formatProposalValue(diff.before);
  return (
    <tr className="border-b border-border/60 last:border-b-0">
      <td className="whitespace-nowrap px-2 py-1 align-top font-mono text-muted-foreground">
        {diff.path}
      </td>
      {showBefore && (
        <td className="px-2 py-1 align-top font-mono text-destructive/80">
          <Cell text={before} />
        </td>
      )}
      <td className="px-2 py-1 align-top font-mono text-success">
        <Cell text={after} />
      </td>
    </tr>
  );
}

/** Cell: long values (scripts, arrays) are clamped to three lines with hover for the full text, so one row cannot blow up the whole table. */
function Cell({ text }: { text: string }) {
  return (
    <span className="line-clamp-3 break-all" title={text}>
      {text}
    </span>
  );
}
