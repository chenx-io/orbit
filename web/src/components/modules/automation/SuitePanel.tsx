// Test suite: suite list on the left, suite detail on the right (member checkboxes / environment / serial-parallel / run).
import { useMemo, useState } from "react";
import {
  ArrowDown,
  ArrowUp,
  Check,
  Folder,
  FolderOpen,
  Layers,
  Loader2,
  Pencil,
  Play,
  Plus,
  Search,
  Trash2,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { ContextMenu } from "./ContextMenu";
import { RenameDialog } from "./RenameDialog";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { PriorityBadge } from "./PriorityBadge";
import { TreeRow } from "@/components/common/Tree";
import { treeFolderIcon, treeMeta } from "@/components/common/treeStyle";
import { buildScenarioTree } from "./treeUtils";

// ── Left column: suite list ────────────────────────────────────

export function SuiteList() {
  const { t } = useT();
  const suites = useAppStore((s) => s.scenarioSuites);
  const activeSuiteId = useAppStore((s) => s.activeSuiteId);
  const setActiveSuite = useAppStore((s) => s.setActiveSuite);
  const addSuite = useAppStore((s) => s.addSuite);
  const updateSuite = useAppStore((s) => s.updateSuite);
  const removeSuite = useAppStore((s) => s.removeSuite);
  const runTarget = useAppStore((s) => s.runTarget);
  const running = useAppStore((s) => s.scenarioRunning);

  const [query, setQuery] = useState("");
  const [dialog, setDialog] = useState<{
    type: "new" | "rename";
    id?: string;
    name?: string;
  } | null>(null);
  const [deleteId, setDeleteId] = useState<string | null>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return suites;
    return suites.filter((s) => s.name.toLowerCase().includes(q));
  }, [suites, query]);

  return (
    <div className="flex h-full flex-col">
      {/* Search + create (header layout consistent with the case tab) */}
      <div className="flex shrink-0 items-center gap-1 border-b border-border p-2">
        <div className="relative flex-1">
          <Search className="absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            placeholder={t("suite.search")}
            className="h-8 pl-7 text-xs"
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <Button
          variant="ghost"
          size="icon-sm"
          className="shrink-0"
          title={t("suite.new")}
          onClick={() => setDialog({ type: "new" })}
        >
          <Plus className="h-4 w-4" />
        </Button>
      </div>
      <ScrollArea className="flex-1">
        <div className="p-1.5">
          {filtered.length === 0 && (
            <div className="px-2 py-8 text-center text-xs text-muted-foreground">
              {suites.length === 0 ? t("suite.empty") : t("suite.noMatch")}
            </div>
          )}
          {filtered.map((su) => {
            const isRunning = running === `suite:${su.id}`;
            return (
              <ContextMenu
                key={su.id}
                items={[
                  {
                    label: t("scenario.runSuite"),
                    icon: Play,
                    action: () => void runTarget({ type: "suite", id: su.id }),
                  },
                  {
                    label: t("suite.rename"),
                    icon: Pencil,
                    action: () =>
                      setDialog({ type: "rename", id: su.id, name: su.name }),
                  },
                  {
                    label: t("suite.delete"),
                    icon: Trash2,
                    danger: true,
                    action: () => setDeleteId(su.id),
                  },
                ]}
              >
                <TreeRow
                  depth={0}
                  selected={activeSuiteId === su.id}
                  onClick={() => setActiveSuite(su.id)}
                  leading={
                    <Layers className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                  }
                  actions={
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="h-6 w-6"
                      title={t("scenario.runSuite")}
                      disabled={!!running}
                      onClick={(e) => {
                        e.stopPropagation();
                        void runTarget({ type: "suite", id: su.id });
                      }}
                    >
                      {isRunning ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <Play className="h-3.5 w-3.5" />
                      )}
                    </Button>
                  }
                >
                  <span className="min-w-0 flex-1 truncate">{su.name}</span>
                  <span className="shrink-0 rounded-full border border-border px-1.5 text-xs text-muted-foreground">
                    {t("report.mode." + su.runMode)}
                  </span>
                  <span className="shrink-0 text-xs text-muted-foreground">
                    {su.memberIds.length}
                  </span>
                </TreeRow>
              </ContextMenu>
            );
          })}
        </div>
      </ScrollArea>

      <RenameDialog
        open={dialog !== null}
        name={dialog?.name ?? ""}
        title={dialog?.type === "new" ? t("suite.new") : t("scenario.rename")}
        onConfirm={(v) => {
          if (!dialog) return;
          if (dialog.type === "new") {
            const id = addSuite(v);
            setActiveSuite(id);
          } else if (dialog.id) {
            updateSuite(dialog.id, { name: v });
          }
          setDialog(null);
        }}
        onCancel={() => setDialog(null)}
        t={t}
      />
      <ConfirmDialog
        open={deleteId !== null}
        onOpenChange={(o) => {
          if (!o) setDeleteId(null);
        }}
        message={t("suite.delete")}
        danger
        confirmLabel={t("common.delete")}
        onConfirm={() => {
          if (deleteId) removeSuite(deleteId);
        }}
      />
    </div>
  );
}

// ── Right column: suite detail ────────────────────────────────

export function SuiteDetail({ suiteId }: { suiteId: string }) {
  const { t, format } = useT();
  const suite = useAppStore((s) =>
    s.scenarioSuites.find((x) => x.id === suiteId),
  );
  const scenarios = useAppStore((s) => s.scenarios);
  const folders = useAppStore((s) => s.scenarioFolders);
  const environments = useAppStore((s) => s.environments);
  const updateSuite = useAppStore((s) => s.updateSuite);
  const setFolderCollapsed = useAppStore((s) => s.setFolderCollapsed);
  const runTarget = useAppStore((s) => s.runTarget);
  const running = useAppStore((s) => s.scenarioRunning);

  const [query, setQuery] = useState("");
  const nodes = useMemo(
    () => buildScenarioTree(scenarios, folders, query),
    [scenarios, folders, query],
  );
  const scenarioById = useMemo(
    () => new Map(scenarios.map((s) => [s.id, s])),
    [scenarios],
  );

  if (!suite) return null;
  const isRunning = running === `suite:${suite.id}`;

  const toggleMember = (id: string): void => {
    const has = suite.memberIds.includes(id);
    updateSuite(suite.id, {
      memberIds: has
        ? suite.memberIds.filter((m) => m !== id)
        : [...suite.memberIds, id],
    });
  };
  const moveMember = (id: string, dir: -1 | 1): void => {
    const idx = suite.memberIds.indexOf(id);
    const next = idx + dir;
    if (idx < 0 || next < 0 || next >= suite.memberIds.length) return;
    const arr = [...suite.memberIds];
    [arr[idx], arr[next]] = [arr[next], arr[idx]];
    updateSuite(suite.id, { memberIds: arr });
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Header: environment + run mode + concurrency + run (suite identity is indicated by the left-column selection) */}
      <div className="flex flex-wrap items-center gap-2 border-b border-border bg-card/30 px-3 py-2">
        <div className="space-y-0.5">
          <div className="text-xs text-muted-foreground">{t("suite.env")}</div>
          <Select
            value={suite.envId ?? "none"}
            onValueChange={(v) =>
              updateSuite(suite.id, { envId: v === "none" ? null : v })
            }
          >
            <SelectTrigger size="sm" className="h-8 w-40 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none" className="text-xs">
                {t("scenario.env.none")}
              </SelectItem>
              {environments.map((e) => (
                <SelectItem key={e.id} value={e.id} className="text-xs">
                  {e.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-0.5">
          <div className="text-xs text-muted-foreground">
            {t("suite.runMode")}
          </div>
          <div className="flex overflow-hidden rounded-md border border-border">
            {(["serial", "parallel"] as const).map((m) => (
              <button
                key={m}
                onClick={() => updateSuite(suite.id, { runMode: m })}
                className={cn(
                  "px-2.5 py-1.5 text-xs transition-colors",
                  suite.runMode === m
                    ? "bg-accent/25 text-foreground"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                {t(`report.mode.${m}`)}
              </button>
            ))}
          </div>
        </div>
        {suite.runMode === "parallel" && (
          <div className="space-y-0.5">
            <div className="text-xs text-muted-foreground">
              {t("suite.concurrency")}
            </div>
            <Input
              type="number"
              min={1}
              max={10}
              value={suite.concurrency ?? 3}
              onChange={(e) =>
                updateSuite(suite.id, {
                  concurrency: Math.max(
                    1,
                    Math.min(10, Number(e.target.value) || 3),
                  ),
                })
              }
              className="h-8 w-16 text-xs"
            />
          </div>
        )}
        <Button
          size="sm"
          className="ml-auto h-8"
          disabled={!!running || suite.memberIds.length === 0}
          onClick={() => void runTarget({ type: "suite", id: suite.id })}
        >
          {isRunning ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Play className="h-3.5 w-3.5" />
          )}
          {t("scenario.run")}
        </Button>
        <div className="w-full text-xs text-muted-foreground">
          {t("suite.envHint")}
        </div>
      </div>

      {/* Members + selectable cases */}
      <div className="flex min-h-0 flex-1">
        {/* Member list */}
        <div className="flex w-64 shrink-0 flex-col border-r border-border">
          <div className="border-b border-border px-2 py-1.5 text-xs text-muted-foreground">
            {t("suite.members")} ·{" "}
            {format("suite.membersCount", suite.memberIds.length)}
          </div>
          <ScrollArea className="flex-1">
            <div className="p-1.5">
              {suite.memberIds.length === 0 && (
                <div className="py-6 text-center text-xs text-muted-foreground">
                  {t("suite.noMembers")}
                </div>
              )}
              {suite.memberIds.map((id, idx) => {
                const sc = scenarioById.get(id);
                return (
                  <TreeRow
                    key={id}
                    depth={0}
                    leading={
                      <span className="w-4 shrink-0 text-xs text-muted-foreground">
                        {idx + 1}
                      </span>
                    }
                    actions={
                      <>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          className="h-6 w-6"
                          onClick={() => moveMember(id, -1)}
                        >
                          <ArrowUp className="h-3 w-3" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          className="h-6 w-6"
                          onClick={() => moveMember(id, 1)}
                        >
                          <ArrowDown className="h-3 w-3" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          className="h-6 w-6"
                          onClick={() => toggleMember(id)}
                        >
                          <X className="h-3 w-3" />
                        </Button>
                      </>
                    }
                  >
                    <PriorityBadge priority={sc?.priority} />
                    <span className="min-w-0 flex-1 truncate">
                      {sc?.name ?? id}
                    </span>
                  </TreeRow>
                );
              })}
            </div>
          </ScrollArea>
        </div>

        {/* Selectable-case tree (checkboxes) */}
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex items-center gap-1 border-b border-border px-2 py-1.5">
            <Input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("scenario.searchCases")}
              className="h-7 flex-1 text-xs"
            />
          </div>
          <ScrollArea className="flex-1">
            <div className="p-1.5">
              <div className="mb-1 px-1.5 text-xs text-muted-foreground">
                {t("suite.hint")}
              </div>
              {nodes.map((node) => {
                if (node.kind === "folder") {
                  return (
                    <TreeRow
                      key={node.id}
                      depth={node.depth}
                      hasChildren
                      collapsed={!!node.collapsed}
                      onToggle={() =>
                        setFolderCollapsed(node.id, !(node.collapsed ?? false))
                      }
                      onClick={() =>
                        setFolderCollapsed(node.id, !(node.collapsed ?? false))
                      }
                      leading={
                        node.collapsed ? (
                          <Folder className={treeFolderIcon} />
                        ) : (
                          <FolderOpen className={treeFolderIcon} />
                        )
                      }
                    >
                      <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">
                        {node.name}
                      </span>
                      <span className={treeMeta}>{node.caseCount}</span>
                    </TreeRow>
                  );
                }
                const checked = suite.memberIds.includes(node.id);
                return (
                  <TreeRow
                    key={node.id}
                    depth={node.depth}
                    selected={checked}
                    onClick={() => toggleMember(node.id)}
                    leading={
                      <span
                        className={cn(
                          "flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-[4px] border",
                          checked
                            ? "border-primary bg-primary text-primary-foreground"
                            : "border-input",
                        )}
                      >
                        {checked && <Check className="h-2.5 w-2.5" />}
                      </span>
                    }
                  >
                    <PriorityBadge priority={node.scenario?.priority} />
                    <span className="min-w-0 flex-1 truncate">{node.name}</span>
                  </TreeRow>
                );
              })}
            </div>
          </ScrollArea>
        </div>
      </div>
    </div>
  );
}
