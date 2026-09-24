// Mock interface editor (interface + expectation model).
// Route key = method + path (from the current request), not editable;
// each interface maps to a set of "expectations" that respond once matched by param conditions + IP condition.
import { useCallback, useEffect, useState } from "react";
import { Pencil, Plus, Trash2, Wand2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { KeyValueEditor } from "@/components/common/KeyValueEditor";
import { DynamicValuePicker } from "@/components/common/DynamicValuePicker";
import { useT } from "@/lib/i18n";
import { saveNow } from "@/lib/persistence";
import { getMockRules, saveMockInterface } from "@/lib/bridge";
import { useAppStore } from "@/store/useStore";
import { uid } from "@/data/seed";
import type {
  HttpRequest,
  KeyValue,
  MockCompareOp,
  MockCondition,
  MockExpectation,
  MockInterface,
  MockIpCondition,
  MockParamLocation,
} from "@/data/types";

const LOCATION_OPTIONS: MockParamLocation[] = [
  "query",
  "path",
  "header",
  "body",
  "cookie",
];

const OP_OPTIONS: MockCompareOp[] = [
  "equals",
  "not_equals",
  "gt",
  "gte",
  "lt",
  "lte",
  "contains",
  "not_contains",
  "exists",
  "not_exists",
  "regex",
];

function headersToKV(h: Record<string, string>): KeyValue[] {
  return Object.entries(h).map(([k, v]) => ({
    id: uid("kv"),
    key: k,
    value: v,
    enabled: true,
  }));
}

function kvToHeaders(items: KeyValue[]): Record<string, string> {
  const h: Record<string, string> = {};
  for (const it of items) {
    if (it.key.trim()) h[it.key.trim()] = it.value;
  }
  return h;
}

function newExpectation(name: string): MockExpectation {
  return {
    id: uid("exp"),
    name,
    enabled: true,
    conditions: [{ location: "query", name: "", op: "equals", value: "" }],
    ipCondition: { enabled: false, ip: "" },
    status: 200,
    headers: {},
    body: '{\n  "ok": true\n}',
    delayMs: 0,
  };
}

function summarizeConditions(
  conditions: MockCondition[],
  ip: MockIpCondition,
  t: (key: string) => string,
): string {
  const parts: string[] = [];
  for (const c of conditions) {
    const loc = t(`mock.loc.${c.location}`);
    const op = t(`mock.op.${c.op}`);
    if (c.op === "exists" || c.op === "not_exists") {
      parts.push(`${loc}.${c.name} ${op}`);
    } else {
      parts.push(`${loc}.${c.name} ${op} "${c.value}"`);
    }
  }
  if (ip.enabled && ip.ip) parts.push(`IP=${ip.ip}`);
  return parts.length ? parts.join("  AND  ") : t("mock.noCondition");
}

function ExpectationCard({
  exp,
  onEdit,
  onToggleEnabled,
  onDelete,
}: {
  exp: MockExpectation;
  onEdit: () => void;
  onToggleEnabled: (enabled: boolean) => void;
  onDelete: () => void;
}) {
  const { t } = useT();
  return (
    <div className="rounded-md border border-border">
      <div className="flex items-center gap-2 px-2.5 py-2">
        <button
          type="button"
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
          onClick={onEdit}
          title={t("mock.edit")}
        >
          <span className="truncate font-mono text-xs font-semibold text-foreground">
            {exp.name || t("mock.unnamed")}
          </span>
          <Badge variant="outline" className="shrink-0 text-xs">
            {exp.status}
          </Badge>
          {exp.delayMs > 0 && (
            <Badge variant="secondary" className="shrink-0 text-xs">
              {exp.delayMs}ms
            </Badge>
          )}
          {exp.ipCondition.enabled && (
            <Badge variant="secondary" className="shrink-0 text-xs">
              IP {exp.ipCondition.ip}
            </Badge>
          )}
        </button>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={onEdit}
          title={t("mock.edit")}
        >
          <Pencil className="h-3.5 w-3.5" />
        </Button>
        <Switch
          checked={exp.enabled}
          onCheckedChange={onToggleEnabled}
          aria-label={t("common.enable") as string}
        />
        <Button
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground hover:text-destructive"
          onClick={onDelete}
          title={t("common.delete") as string}
        >
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
      </div>
      <div className="border-t border-border px-3 py-1.5 font-mono text-xs text-muted-foreground">
        {summarizeConditions(exp.conditions, exp.ipCondition, t)}
      </div>
    </div>
  );
}

// ─── Expectation edit dialog (single entry for add/edit) ───────────────
// Keeps a local copy inside the dialog; only commits to the parent and persists on "Save";
// "Cancel" discards everything and leaves the saved data untouched.
function ExpectationDialog({
  open,
  onOpenChange,
  initial,
  onSave,
  saving,
  title,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  initial: MockExpectation;
  onSave: (next: MockExpectation) => void;
  saving: boolean;
  title: string;
}) {
  const { t } = useT();
  const [exp, setExp] = useState<MockExpectation>(initial);
  const [bodyMode, setBodyMode] = useState<"json" | "raw" | "xml" | "html">(
    "raw",
  );
  const [headers, setHeaders] = useState<KeyValue[]>([]);

  // On open, initialize the local copy from initial (edits do not write back to external state during editing)
  useEffect(() => {
    if (!open) return;
    setExp({
      ...initial,
      conditions: initial.conditions.map((c) => ({ ...c })),
      ipCondition: { ...initial.ipCondition },
      headers: { ...initial.headers },
    });
    setBodyMode(initial.body.trimStart().startsWith("{") ? "json" : "raw");
    setHeaders(headersToKV(initial.headers));
  }, [open, initial]);

  const patch = (p: Partial<MockExpectation>) =>
    setExp((e) => ({ ...e, ...p }));

  const updateCondition = (idx: number, p: Partial<MockCondition>) =>
    patch({
      conditions: exp.conditions.map((c, i) =>
        i === idx ? { ...c, ...p } : c,
      ),
    });
  const addCondition = () =>
    patch({
      conditions: [
        ...exp.conditions,
        { location: "query", name: "", op: "equals", value: "" },
      ],
    });
  const removeCondition = (idx: number) =>
    patch({ conditions: exp.conditions.filter((_, i) => i !== idx) });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[85vh] flex-col overflow-hidden sm:max-w-4xl">
        <DialogHeader className="flex-shrink-0">
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>

        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto pr-3">
          <div>
            <Label className="mb-1 block text-sm">
              {t("mock.expectationName")}
            </Label>
            <Input
              value={exp.name}
              onChange={(e) => patch({ name: e.target.value })}
              className="h-7 text-xs"
              placeholder={t("mock.expectationName")}
            />
          </div>

          {/* Parameter conditions */}
          <div>
            <div className="mb-1 flex items-center justify-between">
              <Label className="text-sm font-semibold">
                {t("mock.conditions")}
              </Label>
              <Button
                variant="ghost"
                size="sm"
                className="h-6 gap-1 px-1.5 text-xs text-muted-foreground"
                onClick={addCondition}
              >
                <Plus className="h-3 w-3" /> {t("mock.addCondition")}
              </Button>
            </div>
            <div className="space-y-1.5">
              {exp.conditions.length === 0 && (
                <div className="text-xs text-muted-foreground">
                  {t("mock.noCondition")}
                </div>
              )}
              {exp.conditions.map((c, idx) => (
                <div key={idx} className="flex items-center gap-1.5">
                  <Select
                    value={c.location}
                    onValueChange={(v) =>
                      updateCondition(idx, { location: v as MockParamLocation })
                    }
                  >
                    <SelectTrigger className="h-7 w-28 text-xs">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {LOCATION_OPTIONS.map((l) => (
                        <SelectItem key={l} value={l}>
                          {t(`mock.loc.${l}`)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Input
                    value={c.name}
                    onChange={(e) =>
                      updateCondition(idx, { name: e.target.value })
                    }
                    className="h-7 w-24 text-xs"
                    placeholder={t("mock.paramName")}
                  />
                  <Select
                    value={c.op}
                    onValueChange={(v) =>
                      updateCondition(idx, { op: v as MockCompareOp })
                    }
                  >
                    <SelectTrigger className="h-7 w-28 text-xs">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {OP_OPTIONS.map((o) => (
                        <SelectItem key={o} value={o}>
                          {t(`mock.op.${o}`)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  {c.op !== "exists" && c.op !== "not_exists" && (
                    <Input
                      value={c.value}
                      onChange={(e) =>
                        updateCondition(idx, { value: e.target.value })
                      }
                      className="h-7 flex-1 text-xs"
                      placeholder={t("mock.compareValue")}
                    />
                  )}
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    className="text-muted-foreground hover:text-destructive"
                    onClick={() => removeCondition(idx)}
                  >
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </div>
              ))}
            </div>
          </div>

          {/* IP conditions */}
          <div className="rounded border border-border/70 p-2">
            <div className="mb-1 flex items-center justify-between">
              <Label className="text-sm font-semibold">
                {t("mock.ipCondition")}
              </Label>
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground">
                  {exp.ipCondition.enabled ? t("mock.ipOnly") : t("mock.ipOff")}
                </span>
                <Switch
                  checked={exp.ipCondition.enabled}
                  onCheckedChange={(v) =>
                    patch({ ipCondition: { ...exp.ipCondition, enabled: v } })
                  }
                />
              </div>
            </div>
            {exp.ipCondition.enabled && (
              <Input
                value={exp.ipCondition.ip}
                onChange={(e) =>
                  patch({
                    ipCondition: { ...exp.ipCondition, ip: e.target.value },
                  })
                }
                className="h-7 text-xs"
                placeholder={t("mock.ipPlaceholder")}
              />
            )}
          </div>

          {/* Response */}
          <div className="flex items-end gap-2">
            <div className="w-20">
              <Label className="mb-1 block text-sm">
                {t("mock.responseStatus")}
              </Label>
              <Input
                type="number"
                value={String(exp.status)}
                onChange={(e) =>
                  patch({ status: parseInt(e.target.value, 10) || 200 })
                }
                className="h-7 text-xs"
              />
            </div>
            <div className="w-24">
              <Label className="mb-1 block text-sm">
                {t("mock.responseDelay")}
              </Label>
              <Input
                type="number"
                value={String(exp.delayMs)}
                onChange={(e) =>
                  patch({ delayMs: parseInt(e.target.value, 10) || 0 })
                }
                className="h-7 text-xs"
              />
            </div>
          </div>

          <div>
            <Label className="mb-1 block text-sm">
              {t("mock.responseHeaders")}
            </Label>
            <KeyValueEditor
              items={headers}
              onChange={(items) => {
                setHeaders(items);
                patch({ headers: kvToHeaders(items) });
              }}
              enableDynamic
            />
          </div>

          <div className="flex items-center gap-2">
            <Label className="text-sm">{t("mock.responseBody")}</Label>
            <Select
              value={bodyMode}
              onValueChange={(v) => setBodyMode(v as typeof bodyMode)}
            >
              <SelectTrigger className="h-7 w-24 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="json">json</SelectItem>
                <SelectItem value="xml">xml</SelectItem>
                <SelectItem value="html">html</SelectItem>
                <SelectItem value="raw">raw</SelectItem>
              </SelectContent>
            </Select>
            {bodyMode === "json" && (
              <Button
                variant="ghost"
                size="sm"
                className="h-7 text-xs"
                onClick={() => {
                  try {
                    patch({
                      body: JSON.stringify(JSON.parse(exp.body), null, 2),
                    });
                  } catch {
                    /* ignore */
                  }
                }}
              >
                <Wand2 className="h-3 w-3" /> {t("mock.format")}
              </Button>
            )}
            {(bodyMode === "json" || bodyMode === "raw") && (
              <DynamicValuePicker
                onInsert={(ins) => patch({ body: exp.body + `{{${ins}}}` })}
              />
            )}
          </div>
          <textarea
            value={exp.body}
            onChange={(e) => patch({ body: e.target.value })}
            className="min-h-27.5 w-full rounded border border-border bg-background p-2 font-mono text-xs text-foreground"
          />
        </div>

        <DialogFooter className="flex-shrink-0">
          <Button
            variant="outline"
            size="sm"
            onClick={() => onOpenChange(false)}
          >
            {t("common.cancel")}
          </Button>
          <Button size="sm" disabled={saving} onClick={() => onSave(exp)}>
            {t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function MockInterfaceEditor({
  req,
  path,
}: {
  req: HttpRequest;
  path: string;
}) {
  const { t } = useT();
  const method = req.method;
  const routeKey = `${method} ${path}`;
  const [interfaces, setInterfaces] = useState<MockInterface[]>([]);
  const [draft, setDraft] = useState<MockInterface | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editingExp, setEditingExp] = useState<MockExpectation>(() =>
    newExpectation(t("mock.newExpectationName")),
  );
  const [saving, setSaving] = useState(false);
  const activeWorkspaceId = useAppStore((s) => s.activeWorkspaceId);

  const reload = useCallback(async () => {
    const list = await getMockRules(activeWorkspaceId ?? undefined);
    setInterfaces(list);
  }, [activeWorkspaceId]);

  // Persist the whole interface draft (expectation add/edit/delete, enable toggle, url sync all go through here; rules take effect live)
  const persist = async (next: MockInterface) => {
    setDraft(next);
    setSaving(true);
    try {
      await saveMockInterface(next);
      await reload();
      // Mock rules live in backend memory and don't trigger frontend store changes; explicitly merge the latest rules into the local snapshot here
      void saveNow();
    } finally {
      setSaving(false);
    }
  };

  // Load the interface list on mount (or when the route key changes)
  useEffect(() => {
    void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [routeKey]);

  // Find the interface linked to the current request: prefer request_id (data survives url edits),
  // fall back to method+path for old data (no request_id)
  const findLinked = (
    list: MockInterface[],
    reqId: string,
    m: string,
    p: string,
  ) =>
    list.find((i) => i.requestId && i.requestId === reqId) ??
    list.find((i) => !i.requestId && i.method === m && i.path === p);

  // Initialize the draft from the current interface (or create a new one if absent)
  useEffect(() => {
    const found = findLinked(interfaces, req.id, method, path);
    if (found) {
      const next: MockInterface = {
        ...found,
        requestId: req.id,
        method,
        path,
      };
      setDraft(next);
      // url/method changed: silently sync method/path in storage while keeping the expectation list
      if (found.method !== method || found.path !== path) {
        void persist(next);
      }
    } else {
      setDraft({
        requestId: req.id,
        workspaceId: activeWorkspaceId ?? undefined,
        method,
        path,
        enabled: true,
        expectations: [],
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [interfaces, routeKey, req.id]);

  // Dialog save: update if id exists, otherwise add
  const handleSaveExp = (next: MockExpectation) => {
    if (!draft) return;
    const exists = draft.expectations.some((e) => e.id === next.id);
    void persist({
      ...draft,
      expectations: exists
        ? draft.expectations.map((e) => (e.id === next.id ? next : e))
        : [...draft.expectations, next],
    });
    setDialogOpen(false);
  };

  const handleDeleteExp = (id: string) => {
    if (!draft) return;
    void persist({
      ...draft,
      expectations: draft.expectations.filter((e) => e.id !== id),
    });
  };

  const handleToggleEnabled = (id: string, enabled: boolean) => {
    if (!draft) return;
    void persist({
      ...draft,
      expectations: draft.expectations.map((e) =>
        e.id === id ? { ...e, enabled } : e,
      ),
    });
  };

  const openCreate = () => {
    setEditingExp(newExpectation(t("mock.newExpectationName")));
    setDialogOpen(true);
  };
  const openEdit = (exp: MockExpectation) => {
    setEditingExp(exp);
    setDialogOpen(true);
  };

  if (!draft) return null;

  return (
    <div className="space-y-3">
      <Button
        variant="outline"
        size="sm"
        className="w-full gap-1 text-xs"
        onClick={openCreate}
      >
        <Plus className="h-3 w-3" /> {t("mock.addExpectation")}
      </Button>
      <div className="space-y-2">
        {draft.expectations.map((exp) => (
          <ExpectationCard
            key={exp.id}
            exp={exp}
            onEdit={() => openEdit(exp)}
            onToggleEnabled={(v) => handleToggleEnabled(exp.id, v)}
            onDelete={() => handleDeleteExp(exp.id)}
          />
        ))}
      </div>

      <ExpectationDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        initial={editingExp}
        onSave={handleSaveExp}
        saving={saving}
        title={
          draft.expectations.some((e) => e.id === editingExp.id)
            ? t("mock.editExpectation")
            : t("mock.addExpectation")
        }
      />
    </div>
  );
}
