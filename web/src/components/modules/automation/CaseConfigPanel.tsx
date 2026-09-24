// Case run configuration (inline collapsible panel docked on the right of the editor).
// Shares the same horizontal PanelGroup with the "step list": expanded it shrinks the step area, collapsed the step area takes full width.
// Fields mirror the old Drawer/dialog: priority / run environment / data-driven / loop count / failure policy / description.
import { Settings2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
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
import type { Scenario, ScenarioPriority } from "@/data/types";
import { FAILURE_POLICIES } from "@/store/utils/scenarioRunner";

const PRIORITIES: (ScenarioPriority | "none")[] = [
  "none",
  "p0",
  "p1",
  "p2",
  "p3",
];

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <div className="text-xs font-medium text-muted-foreground">{label}</div>
      {children}
      {hint && <div className="text-xs text-muted-foreground/80">{hint}</div>}
    </div>
  );
}

export function CaseConfigPanel({
  scenario,
  onManageDataSets,
  onClose,
}: {
  scenario: Scenario;
  onManageDataSets: () => void;
  onClose: () => void;
}) {
  const { t } = useT();
  const environments = useAppStore((s) => s.environments);
  const dataSets = useAppStore((s) => s.scenarioDataSets);
  const update = useAppStore((s) => s.updateScenario);

  const useData = scenario.useDataSet === true;

  return (
    <div className="flex h-full min-h-0 flex-col border-l border-border bg-background">
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2">
        <Settings2 className="h-4 w-4 shrink-0 text-muted-foreground" />
        <span className="text-sm font-medium">{t("scenario.runConfig")}</span>
        <Button
          variant="ghost"
          size="icon-sm"
          className="ml-auto h-7 w-7"
          title={t("common.close")}
          onClick={onClose}
        >
          <X className="h-4 w-4" />
        </Button>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        <div className="space-y-3.5 p-3">
          <Field label={t("scenario.priority")}>
            <Select
              value={scenario.priority ?? "none"}
              onValueChange={(v) =>
                update(scenario.id, {
                  priority: v === "none" ? undefined : (v as ScenarioPriority),
                })
              }
            >
              <SelectTrigger size="sm" className="h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PRIORITIES.map((p) => (
                  <SelectItem key={p} value={p} className="text-xs">
                    {p === "none"
                      ? t("scenario.priority.none")
                      : p.toUpperCase()}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>

          <Field label={t("scenario.env")}>
            <Select
              value={scenario.envId ?? "none"}
              onValueChange={(v) =>
                update(scenario.id, { envId: v === "none" ? null : v })
              }
            >
              <SelectTrigger size="sm" className="h-8 text-xs">
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
          </Field>

          <Field label={t("scenario.useDataSet")}>
            <div className="flex items-center gap-2">
              <Switch
                checked={useData}
                onCheckedChange={(v) => update(scenario.id, { useDataSet: v })}
                aria-label={t("scenario.useDataSet")}
              />
              <Select
                value={scenario.dataSetId ?? "none"}
                disabled={!useData}
                onValueChange={(v) =>
                  update(scenario.id, { dataSetId: v === "none" ? null : v })
                }
              >
                <SelectTrigger size="sm" className="h-8 min-w-0 flex-1 text-xs">
                  <SelectValue placeholder={t("scenario.dataset.none")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="none" className="text-xs">
                    {t("scenario.dataset.none")}
                  </SelectItem>
                  {dataSets.map((d) => (
                    <SelectItem key={d.id} value={d.id} className="text-xs">
                      {d.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Button
                variant="ghost"
                size="icon-sm"
                className="h-8 w-8 shrink-0"
                title={t("scenario.manageDataSets")}
                onClick={onManageDataSets}
              >
                <Settings2 className="h-3.5 w-3.5" />
              </Button>
            </div>
            {useData && (
              <div className="text-xs text-muted-foreground/80">
                {t("scenario.iterationsHint")}
              </div>
            )}
          </Field>

          <Field label={t("scenario.iterations")}>
            <Input
              type="number"
              min={1}
              max={9999}
              value={scenario.iterations ?? 1}
              onChange={(e) =>
                update(scenario.id, {
                  iterations: Math.max(1, Number(e.target.value) || 1),
                })
              }
              className="h-8 w-20 text-xs"
            />
          </Field>

          <Field label={t("scenario.onError")}>
            <Select
              value={scenario.onError ?? "stop"}
              onValueChange={(v) =>
                update(scenario.id, { onError: v as never })
              }
            >
              <SelectTrigger size="sm" className="h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {FAILURE_POLICIES.map((p) => (
                  <SelectItem key={p} value={p} className="text-xs">
                    {t(
                      `scenario.onError.${p === "next-loop" ? "nextLoop" : p}`,
                    )}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>

          <Field label={t("scenario.recordRequestDetails")}>
            <div className="flex items-center gap-2">
              <Switch
                checked={scenario.recordRequestDetails === true}
                onCheckedChange={(v) =>
                  update(scenario.id, { recordRequestDetails: v })
                }
                aria-label={t("scenario.recordRequestDetails")}
              />
              <span className="text-xs text-muted-foreground/80">
                {t("scenario.recordRequestDetailsHint")}
              </span>
            </div>
          </Field>

          <Field label={t("scenario.description")}>
            <Input
              value={scenario.description ?? ""}
              placeholder={t("scenario.descriptionPlaceholder")}
              onChange={(e) =>
                update(scenario.id, {
                  description: e.target.value || undefined,
                })
              }
              className="h-8 text-xs"
            />
          </Field>
        </div>
      </ScrollArea>
    </div>
  );
}
