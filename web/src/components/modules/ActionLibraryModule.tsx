// Script library management (a full-width module): maintain reusable action templates in one place and reference them from requests.
//
// The layout follows the design spec's "list left, editor right" pattern: the list on the left (search / reference counts / one "new" entry)
// and the editor on the right, headed by the current object and footer-listing the requests that reference it. Library items are isolated per workspace.
//
// **Draft state**: edits touch only a local copy; "save" writes back to the store (taking effect for referencing requests immediately) and "cancel" discards.
// Switching items or creating one with unsaved changes asks first (save / discard), so freshly typed code is never lost silently.
import { useEffect, useState } from "react";
import {
  BookOpen,
  Check,
  ChevronDown,
  Code2,
  Copy,
  Database,
  Plus,
  Search,
  Trash2,
  Undo2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { useAppStore } from "@/store/useStore";
import {
  filterTemplates,
  isTemplateDirty,
  newActionTemplate,
  templateUsages,
} from "@/lib/actionLibrary";
import type { DataSourceOption } from "@/lib/dbQuery";
import type { ActionTemplate } from "@/data/types";
import { ScriptEditor } from "./interface/ScriptEditor";
import { DbActionForm } from "./interface/actions/DbActionForm";

/** Draft being edited: `isNew` means not yet persisted (not referenceable until saved, gone on close) */
interface Draft {
  item: ActionTemplate;
  isNew: boolean;
}

/** Pending navigation with unsaved changes (ask the user how to handle the draft first) */
type Pending =
  | { kind: "switch"; id: string | null }
  | { kind: "new"; ctor: "script" | "db" };

/** Deep-copy a library item as the draft: edits touch only the copy, so unsaved changes never leak to referencing requests */
function cloneTemplate(tpl: ActionTemplate): ActionTemplate {
  return JSON.parse(JSON.stringify(tpl)) as ActionTemplate;
}

export function ActionLibraryModule() {
  const { t } = useT();
  const templates = useAppStore((s) => s.actionTemplates);
  const requests = useAppStore((s) => s.requests);
  const dataSources = useAppStore((s) => s.dataSources) as DataSourceOption[];
  const addActionTemplate = useAppStore((s) => s.addActionTemplate);
  const updateActionTemplate = useAppStore((s) => s.updateActionTemplate);
  const removeActionTemplate = useAppStore((s) => s.removeActionTemplate);
  const duplicateActionTemplate = useAppStore((s) => s.duplicateActionTemplate);

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [query, setQuery] = useState("");
  const [pendingDelete, setPendingDelete] = useState<ActionTemplate | null>(
    null,
  );

  const saved = templates.find((x) => x.id === selectedId) ?? null;
  const dirty =
    !!draft && (draft.isNew || !saved || isTemplateDirty(draft.item, saved));
  const filtered = filterTemplates(templates, query);

  // Clear a dangling draft when the item was deleted elsewhere (or the workspace changed), so "save" cannot write into nothing
  useEffect(() => {
    if (selectedId && !templates.some((x) => x.id === selectedId)) {
      setSelectedId(null);
      setDraft(null);
    }
  }, [templates, selectedId]);

  /** Empty shell for a new library item (script / database query) */
  const newItem = (kind: "script" | "db"): ActionTemplate => {
    const base = newActionTemplate(t("lib.untitled"));
    if (kind === "script") return base;
    return {
      ...base,
      action: {
        id: base.action.id,
        kind: "db",
        name: "",
        enabled: true,
        datasource: dataSources[0]?.id ?? "",
        sql: "SELECT 1",
        target: { type: "scalar" },
        extractVar: "",
        columns: [],
        row: 0,
      },
    };
  };

  /** Read the freshest library table: items written in the same batch (after a duplicate / save) must be visible too */
  const latestTemplates = () => useAppStore.getState().actionTemplates;

  /** Load the **saved** content of a library item (an empty id returns to the empty state) */
  const openSaved = (id: string | null) => {
    const tpl = id ? latestTemplates().find((x) => x.id === id) : undefined;
    setSelectedId(tpl ? tpl.id : null);
    setDraft(tpl ? { item: cloneTemplate(tpl), isNew: false } : null);
  };

  const goTo = (p: Pending | null) => {
    if (!p) {
      setSelectedId(null);
      setDraft(null);
    } else if (p.kind === "new") {
      setSelectedId(null);
      setDraft({ item: newItem(p.ctor), isNew: true });
    } else {
      openSaved(p.id);
    }
  };

  /** Ask when there are unsaved changes, otherwise navigate straight away */
  const guardDirty = (p: Pending) => {
    if (dirty) setPending(p);
    else goTo(p);
  };

  const patchDraft = (updates: Partial<ActionTemplate>) =>
    setDraft((d) => (d ? { ...d, item: { ...d.item, ...updates } } : d));

  /** Save the draft: a new item is persisted and selected, an existing one is updated in place (referencing requests read the new content at once) */
  const save = () => {
    if (!draft) return;
    const name = draft.item.name.trim() || t("lib.untitled");
    if (draft.isNew) {
      const id = addActionTemplate({
        name,
        description: draft.item.description,
        action: draft.item.action,
      });
      setSelectedId(id);
      setDraft({ item: { ...draft.item, id, name }, isNew: false });
      return;
    }
    updateActionTemplate(draft.item.id, {
      name,
      description: draft.item.description,
      action: draft.item.action,
    });
    setSelectedId(draft.item.id);
    setDraft({ item: { ...draft.item, name }, isNew: false });
  };

  /** Cancel edits: a saved item reverts to its last saved content and a new draft is discarded */
  const cancel = () => {
    if (draft?.isNew) goTo(null);
    else openSaved(selectedId);
  };

  const usages = draft ? templateUsages(draft.item.id, requests) : [];

  return (
    <div className="flex h-full min-h-0 flex-1 overflow-hidden">
      {/* Left: list */}
      <div className="flex w-72 shrink-0 flex-col border-r border-border">
        <div className="space-y-2 border-b border-border p-3">
          <div className="flex items-center gap-2">
            <BookOpen className="h-4 w-4 text-violet-400" />
            <span className="text-sm font-semibold">{t("lib.title")}</span>
            <span className="ml-auto text-xs text-muted-foreground">
              {templates.length}
            </span>
          </div>
          <div className="relative">
            <Search className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              className="h-8 pl-7 text-xs"
              placeholder={t("lib.searchPlaceholder")}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          {/* A single "new" entry, with script / query chosen from the menu */}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="outline"
                size="sm"
                className="h-7 w-full justify-start gap-1 px-2 text-xs"
              >
                <Plus className="h-3 w-3" /> {t("lib.new")}
                <ChevronDown className="ml-auto h-3 w-3 opacity-60" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-44">
              <DropdownMenuItem
                className="text-xs"
                onSelect={() => guardDirty({ kind: "new", ctor: "script" })}
              >
                <Code2 className="h-3.5 w-3.5 text-fuchsia-400" />
                {t("lib.newScript")}
              </DropdownMenuItem>
              <DropdownMenuItem
                className="text-xs"
                onSelect={() => guardDirty({ kind: "new", ctor: "db" })}
              >
                <Database className="h-3.5 w-3.5 text-sky-400" />
                {t("lib.newDb")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {/* An unsaved new draft: it takes its place in the list so the user can tell what is being edited */}
          {draft?.isNew && (
            <div className="relative flex w-full items-center gap-2 rounded-md bg-accent py-1.5 pl-2.5 pr-2">
              <span className="absolute left-0 top-1.5 h-[calc(100%-12px)] w-0.5 rounded-full bg-violet-400" />
              {draft.item.action.kind === "script" ? (
                <Code2 className="h-3.5 w-3.5 shrink-0 text-fuchsia-400" />
              ) : (
                <Database className="h-3.5 w-3.5 shrink-0 text-sky-400" />
              )}
              <span className="min-w-0 flex-1 truncate text-xs">
                {draft.item.name || t("lib.untitled")}
              </span>
              <span className="shrink-0 text-xs text-amber-400">
                {t("lib.unsaved")}
              </span>
            </div>
          )}
          {templates.length === 0 && !draft?.isNew && (
            <div className="px-2 py-6 text-center text-xs text-muted-foreground">
              {t("lib.empty")}
            </div>
          )}
          {templates.length > 0 && filtered.length === 0 && (
            <div className="px-2 py-6 text-center text-xs text-muted-foreground">
              {t("lib.noMatch")}
            </div>
          )}
          {filtered.map((tpl) => {
            const used = templateUsages(tpl.id, requests).length;
            const active = tpl.id === selectedId;
            return (
              <button
                key={tpl.id}
                type="button"
                onClick={() => {
                  // Already the current item: do nothing (reloading would wipe a half-typed draft)
                  if (tpl.id === selectedId) return;
                  guardDirty({ kind: "switch", id: tpl.id });
                }}
                className={cn(
                  "relative flex w-full items-center gap-2 rounded-md py-1.5 pl-2.5 pr-2 text-left transition-colors",
                  active ? "bg-accent" : "hover:bg-accent/15",
                )}
              >
                {active && (
                  <span className="absolute left-0 top-1.5 h-[calc(100%-12px)] w-0.5 rounded-full bg-violet-400" />
                )}
                {tpl.action.kind === "script" ? (
                  <Code2 className="h-3.5 w-3.5 shrink-0 text-fuchsia-400" />
                ) : (
                  <Database className="h-3.5 w-3.5 shrink-0 text-sky-400" />
                )}
                <span className="min-w-0 flex-1 truncate text-xs">
                  {tpl.name}
                </span>
                {active && dirty && (
                  <span
                    className="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-400"
                    title={t("lib.unsaved")}
                  />
                )}
                {used > 0 && (
                  <span className="shrink-0 text-xs text-muted-foreground">
                    {used}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      </div>

      {/* Right: editor (content comes entirely from the draft and is not written to the store before saving) */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {!draft ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
            <BookOpen className="h-6 w-6" />
            <span className="text-xs">{t("lib.selectHint")}</span>
          </div>
        ) : (
          <div className="mx-auto max-w-3xl space-y-4 p-4">
            <div className="flex items-center gap-2">
              <span className="truncate text-lg font-semibold">
                {draft.item.name || t("lib.untitled")}
              </span>
              <Badge variant="outline" className="shrink-0 text-xs">
                {draft.item.action.kind === "script"
                  ? t("action.kindScript")
                  : t("action.kindDb")}
              </Badge>
              {dirty && (
                <Badge
                  variant="outline"
                  className="shrink-0 border-amber-500/40 text-xs text-amber-400"
                >
                  {t("lib.unsaved")}
                </Badge>
              )}
              <div className="ml-auto flex items-center gap-1.5">
                <Button
                  size="sm"
                  className="h-7 gap-1 px-2 text-xs"
                  disabled={!dirty}
                  onClick={save}
                >
                  <Check className="h-3.5 w-3.5" /> {t("lib.save")}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 gap-1 px-2 text-xs"
                  disabled={!dirty}
                  onClick={cancel}
                >
                  <Undo2 className="h-3.5 w-3.5" /> {t("lib.cancel")}
                </Button>
                {/* Duplicate / delete only affect the **saved** version: with unsaved changes you must save or cancel first, so the semantics stay unambiguous */}
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 gap-1 px-2 text-xs"
                  disabled={dirty}
                  onClick={() => {
                    const id = duplicateActionTemplate(draft.item.id);
                    if (id) openSaved(id);
                  }}
                >
                  <Copy className="h-3.5 w-3.5" /> {t("action.duplicate")}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 gap-1 px-2 text-xs text-rose-400 hover:text-rose-300"
                  disabled={dirty}
                  onClick={() => setPendingDelete(draft.item)}
                >
                  <Trash2 className="h-3.5 w-3.5" /> {t("action.delete")}
                </Button>
              </div>
            </div>

            <div className="space-y-1.5">
              <Label className="text-sm">{t("lib.name")}</Label>
              <Input
                className="h-8 text-xs"
                value={draft.item.name}
                onChange={(e) => patchDraft({ name: e.target.value })}
              />
            </div>

            <div className="space-y-1.5">
              <Label className="text-sm">{t("lib.description")}</Label>
              <Textarea
                className="h-16 text-xs"
                placeholder={t("lib.descriptionPlaceholder")}
                value={draft.item.description ?? ""}
                onChange={(e) => patchDraft({ description: e.target.value })}
              />
            </div>

            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <Label className="text-sm">{t("lib.content")}</Label>
                <div className="ml-auto flex items-center gap-1.5">
                  <Button
                    variant={
                      draft.item.action.kind === "script"
                        ? "secondary"
                        : "ghost"
                    }
                    size="sm"
                    className="h-6 px-2 text-xs"
                    onClick={() =>
                      patchDraft({
                        // Changing the type swaps only the action content and keeps the original id (a stable key for the draft / after saving)
                        action: {
                          ...newItem("script").action,
                          id: draft.item.action.id,
                        },
                      })
                    }
                  >
                    {t("action.kindScript")}
                  </Button>
                  <Button
                    variant={
                      draft.item.action.kind === "db" ? "secondary" : "ghost"
                    }
                    size="sm"
                    className="h-6 px-2 text-xs"
                    onClick={() =>
                      patchDraft({
                        action: {
                          ...newItem("db").action,
                          id: draft.item.action.id,
                        },
                      })
                    }
                  >
                    {t("action.kindDb")}
                  </Button>
                </div>
              </div>
              {draft.item.action.kind === "script" ? (
                <ScriptEditor
                  value={draft.item.action.code}
                  onChange={(code) => {
                    const action = draft.item.action;
                    if (action.kind !== "script") return;
                    patchDraft({ action: { ...action, code } });
                  }}
                  kind="pre"
                />
              ) : (
                <DbActionForm
                  value={draft.item.action}
                  onChange={(patch) => {
                    const action = draft.item.action;
                    if (action.kind !== "db") return;
                    patchDraft({ action: { ...action, ...patch } });
                  }}
                  dataSources={dataSources}
                />
              )}
            </div>

            <div className="space-y-1.5">
              <Label className="text-sm">
                {t("lib.usage").replace("{0}", String(usages.length))}
              </Label>
              {usages.length === 0 ? (
                <div className="text-xs text-muted-foreground">
                  {t("lib.usageEmpty")}
                </div>
              ) : (
                <div className="space-y-0.5">
                  {usages.map((u) => (
                    <div
                      key={u.requestId}
                      className="flex items-center gap-2 text-xs"
                    >
                      <span className="truncate">{u.requestName}</span>
                      <span className="shrink-0 text-muted-foreground">
                        ×{u.count}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      {/* Unsaved-change guard: ask before switching / creating, so freshly typed content is never lost silently */}
      <Dialog open={!!pending} onOpenChange={(o) => !o && setPending(null)}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle className="text-lg">{t("lib.dirtyTitle")}</DialogTitle>
            <DialogDescription className="text-xs">
              {t("lib.dirtyDesc")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              size="sm"
              onClick={() => {
                const target = pending;
                save();
                setPending(null);
                goTo(target);
              }}
            >
              {t("lib.save")}
            </Button>
            <Button
              variant="destructive"
              size="sm"
              onClick={() => {
                const target = pending;
                setPending(null);
                goTo(target);
              }}
            >
              {t("lib.discard")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete confirmation: states which requests are affected (deleting an item does not clean up references) */}
      <Dialog
        open={!!pendingDelete}
        onOpenChange={(o) => !o && setPendingDelete(null)}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle className="text-lg">
              {t("lib.deleteTitle")}
            </DialogTitle>
            <DialogDescription className="text-xs">
              {t("lib.deleteDesc").replace("{0}", pendingDelete?.name ?? "")}
            </DialogDescription>
          </DialogHeader>
          <div className="text-xs text-muted-foreground">
            {t("lib.deleteImpact").replace(
              "{0}",
              String(
                pendingDelete
                  ? templateUsages(pendingDelete.id, requests).length
                  : 0,
              ),
            )}
          </div>
          <DialogFooter>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setPendingDelete(null)}
            >
              {t("lib.cancel")}
            </Button>
            <Button
              variant="destructive"
              size="sm"
              onClick={() => {
                if (pendingDelete) removeActionTemplate(pendingDelete.id);
                setSelectedId(null);
                setDraft(null);
                setPendingDelete(null);
              }}
            >
              {t("action.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
