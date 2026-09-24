// References (the context boundary): a chips row plus a "+" category menu.
//
// Both entry points share one set of state and candidates:
// - the chips row above the input: view/remove pinned objects (`ReferenceChips`);
// - the bottom bar's "+": browse by category (workspace/collection/request/scenario/external definition file);
// - typing `@` in the input: flat search (implemented in the Composer, see MentionMenu).
import {
  Boxes,
  FileCode,
  FolderTree,
  ListChecks,
  Plus,
  Webhook,
  X,
} from "lucide-react";
import type { AiReference, AiReferenceKind } from "@/data/aiTypes";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { tFormat } from "@/lib/localeDict";
import { describeRequest, referenceKindLabelKey } from "@/lib/ai/references";
import { useAppStore } from "@/store/useStore";

const KIND_ICONS: Record<AiReferenceKind, typeof Boxes> = {
  workspace: Boxes,
  collection: FolderTree,
  request: Webhook,
  scenario: ListChecks,
  file: FileCode,
};

/** How many items a submenu lists at most (beyond that `@` search is the way). */
const SUBMENU_LIMIT = 200;

/** Pinned references: a row of chips, removable one by one. Takes no space when empty. */
export function ReferenceChips() {
  const references = useAppStore((s) => s.aiReferences);
  const removeReference = useAppStore((s) => s.aiRemoveReference);
  if (references.length === 0) return null;
  return (
    <div className="mb-1.5 flex flex-wrap items-center gap-1">
      {references.map((ref) => (
        <ReferenceChip
          key={`${ref.kind}:${ref.id}`}
          reference={ref}
          onRemove={() => removeReference(ref.kind, ref.id)}
        />
      ))}
    </div>
  );
}

/** The bottom bar's "+": add a reference by category (including the external definition file dialog). */
export function ReferenceAddButton() {
  const { t } = useT();
  const addReference = useAppStore((s) => s.aiAddReference);
  const attachFile = useAppStore((s) => s.aiAttachDefinitionFile);

  const workspaces = useAppStore((s) => s.workspaces);
  const activeWorkspaceId = useAppStore((s) => s.activeWorkspaceId);
  const collections = useAppStore((s) => s.collections);
  const requests = useAppStore((s) => s.requests);
  const scenarios = useAppStore((s) => s.scenarios);

  const workspace = workspaces.find((w) => w.id === activeWorkspaceId) ?? null;
  const requestRows = Object.values(requests).slice(0, SUBMENU_LIMIT);
  const scenarioRows = scenarios.slice(0, SUBMENU_LIMIT);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          title={t("ai.reference.add")}
          className="flex size-7 shrink-0 cursor-pointer items-center justify-center rounded-full border border-border text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        >
          <Plus className="h-3.5 w-3.5" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="start" className="w-64">
        <DropdownMenuLabel>{t("ai.reference.title")}</DropdownMenuLabel>
        <DropdownMenuSeparator />

        <DropdownMenuItem
          disabled={!workspace}
          onSelect={() => {
            if (!workspace) return;
            addReference({
              kind: "workspace",
              id: workspace.id,
              name: workspace.name,
            });
          }}
        >
          <Boxes className="h-3.5 w-3.5" />
          {t("ai.reference.workspace")}
          {workspace && (
            <span className="ml-auto max-w-24 truncate text-xs text-muted-foreground">
              {workspace.name}
            </span>
          )}
        </DropdownMenuItem>

        <EntitySub
          kind="collection"
          label={t("ai.reference.collection")}
          empty={t("ai.reference.noCollections")}
          rows={collections.map((c) => ({ id: c.id, name: c.name }))}
          onPick={(row) =>
            addReference({ kind: "collection", id: row.id, name: row.name })
          }
        />

        <EntitySub
          kind="request"
          label={t("ai.reference.request")}
          empty={t("ai.reference.noRequests")}
          rows={requestRows.map((r) => ({
            id: r.id,
            name: r.name || r.id,
            detail: describeRequest(r),
          }))}
          onPick={(row) =>
            addReference({
              kind: "request",
              id: row.id,
              name: row.name,
              detail: row.detail,
            })
          }
        />

        <EntitySub
          kind="scenario"
          label={t("ai.reference.scenario")}
          empty={t("ai.reference.noScenarios")}
          rows={scenarioRows.map((s) => ({
            id: s.id,
            name: s.name,
            detail: `${s.steps.length}`,
          }))}
          onPick={(row) =>
            addReference({
              kind: "scenario",
              id: row.id,
              name: row.name,
              detail: row.detail,
            })
          }
        />

        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => void attachFile()}>
          <FileCode className="h-3.5 w-3.5" />
          {t("ai.reference.file")}
          <span className="ml-auto text-xs text-muted-foreground">
            {t("ai.reference.fileHint")}
          </span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function referenceDetail(ref: AiReference): string {
  if (ref.kind === "file") {
    return ref.truncated
      ? tFormat("ai.ref.truncated", ref.detail ?? "").trim()
      : (ref.detail ?? "");
  }
  return ref.detail ?? "";
}

function ReferenceChip({
  reference,
  onRemove,
}: {
  reference: AiReference;
  onRemove: () => void;
}) {
  const { t } = useT();
  const Icon = KIND_ICONS[reference.kind];
  const detail = referenceDetail(reference);
  // A truncated file must be flagged explicitly: the model sees an incomplete definition and the user has to know
  const warn = reference.kind === "file" && reference.truncated;

  return (
    <span
      title={`${t(referenceKindLabelKey(reference.kind))}: ${reference.name}${detail ? ` · ${detail}` : ""}`}
      className={cn(
        "flex max-w-full items-center gap-1 rounded-md border px-1.5 py-0.5 text-xs",
        warn
          ? "border-warning/50 bg-warning/10 text-warning"
          : "border-border bg-muted text-muted-foreground",
      )}
    >
      <Icon className="h-3 w-3 shrink-0" />
      <span className="max-w-40 truncate">{reference.name}</span>
      {detail && (
        <span className="max-w-32 truncate font-mono text-xs opacity-70">
          {detail}
        </span>
      )}
      <button
        type="button"
        title={t("ai.context.clear")}
        className="shrink-0 cursor-pointer hover:text-foreground"
        onClick={onRemove}
      >
        <X className="h-3 w-3" />
      </button>
    </span>
  );
}

interface EntityRow {
  id: string;
  name: string;
  detail?: string;
}

/** Submenu for one entity kind (collections/requests/scenarios). */
function EntitySub({
  kind,
  label,
  empty,
  rows,
  onPick,
}: {
  kind: AiReferenceKind;
  label: string;
  empty: string;
  rows: EntityRow[];
  onPick: (row: EntityRow) => void;
}) {
  const Icon = KIND_ICONS[kind];
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        <Icon className="h-3.5 w-3.5" />
        <span className="min-w-0 flex-1 truncate">{label}</span>
        <span className="text-xs text-muted-foreground">{rows.length}</span>
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent
        sideOffset={4}
        className="max-h-72 w-72 overflow-y-auto"
      >
        {rows.length === 0 && (
          <DropdownMenuItem disabled>{empty}</DropdownMenuItem>
        )}
        {rows.map((row) => (
          <DropdownMenuItem key={row.id} onSelect={() => onPick(row)}>
            <span className="min-w-0 flex-1 truncate">{row.name}</span>
            {row.detail && (
              <span className="max-w-32 truncate font-mono text-xs text-muted-foreground">
                {row.detail}
              </span>
            )}
          </DropdownMenuItem>
        ))}
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  );
}
