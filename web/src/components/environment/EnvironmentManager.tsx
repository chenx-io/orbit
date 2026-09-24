import { useEffect, useState } from "react";
import { Globe, Plus, Trash2, Check, KeyRound, Layers } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import type { Environment } from "@/data/types";
import { cn } from "@/lib/utils";

type Pair = { key: string; value: string };

/** Panel target: global variables / global secrets / a specific environment */
type PanelTarget =
  | { kind: "global-vars" }
  | { kind: "global-secrets" }
  | { kind: "env"; id: string };

function toPairs(rec: Record<string, string>): Pair[] {
  return Object.entries(rec).map(([key, value]) => ({ key, value }));
}
function toRecord(pairs: Pair[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const p of pairs) if (p.key.trim()) out[p.key.trim()] = p.value;
  return out;
}

export function EnvironmentManager() {
  const open = useAppStore((s) => s.ui.envEditorOpen);
  const setOpen = useAppStore((s) => s.setEnvEditorOpen);
  const environments = useAppStore((s) => s.environments);
  const activeEnvId = useAppStore((s) => s.activeEnvId);
  const globalVariables = useAppStore((s) => s.globalVariables);
  const globalSecrets = useAppStore((s) => s.globalSecrets);
  const addEnvironment = useAppStore((s) => s.addEnvironment);
  const renameEnvironment = useAppStore((s) => s.renameEnvironment);
  const removeEnvironment = useAppStore((s) => s.removeEnvironment);
  const updateEnvironment = useAppStore((s) => s.updateEnvironment);
  const updateGlobalVariables = useAppStore((s) => s.updateGlobalVariables);
  const updateGlobalSecrets = useAppStore((s) => s.updateGlobalSecrets);
  const setActiveEnv = useAppStore((s) => s.setActiveEnv);
  const { t } = useT();

  const [target, setTarget] = useState<PanelTarget>(
    activeEnvId ? { kind: "env", id: activeEnvId } : { kind: "global-vars" },
  );
  const [vars, setVars] = useState<Pair[]>([]);
  const [secrets, setSecrets] = useState<Pair[]>([]);
  const [name, setName] = useState("");

  const selectedEnv: Environment | undefined =
    target.kind === "env"
      ? environments.find((e) => e.id === target.id)
      : undefined;

  // Reset the target when opening (the currently active environment wins)
  useEffect(() => {
    if (!open) return;
    setTarget(
      activeEnvId ? { kind: "env", id: activeEnvId } : { kind: "global-vars" },
    );
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  // Target changed → sync the editor content
  useEffect(() => {
    if (target.kind === "global-vars") {
      setVars(toPairs(globalVariables));
      setSecrets([]);
      setName("");
    } else if (target.kind === "global-secrets") {
      setVars([]);
      setSecrets(toPairs(globalSecrets));
      setName("");
    } else if (selectedEnv) {
      setVars(toPairs(selectedEnv.variables));
      setSecrets(toPairs(selectedEnv.secrets));
      setName(selectedEnv.name);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target, selectedEnv?.id]);

  const commitVars = (next: Pair[]) => {
    setVars(next);
    if (target.kind === "global-vars") {
      updateGlobalVariables(toRecord(next));
    } else if (selectedEnv) {
      updateEnvironment(selectedEnv.id, { variables: toRecord(next) });
    }
  };
  const commitSecrets = (next: Pair[]) => {
    setSecrets(next);
    if (target.kind === "global-secrets") {
      updateGlobalSecrets(toRecord(next));
    } else if (selectedEnv) {
      updateEnvironment(selectedEnv.id, { secrets: toRecord(next) });
    }
  };

  const targetMeta =
    target.kind === "global-vars"
      ? {
          icon: <Layers className="h-4 w-4" />,
          title: t("env.globalVars"),
          subtitle: t("env.globalVarsDesc"),
        }
      : target.kind === "global-secrets"
        ? {
            icon: <KeyRound className="h-4 w-4" />,
            title: t("env.globalSecrets"),
            subtitle: t("env.globalSecretsDesc"),
          }
        : selectedEnv
          ? {
              icon: <Globe className="h-4 w-4" />,
              title: selectedEnv.name,
              subtitle: t("env.envSubtitle"),
            }
          : null;

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Globe className="h-4 w-4 text-primary" /> {t("env.title")}
          </DialogTitle>
          <DialogDescription>{t("env.desc")}</DialogDescription>
        </DialogHeader>

        <div className="flex h-105 gap-3">
          {/* Left: globals + environment list */}
          <div className="w-44 shrink-0 overflow-auto rounded-lg border border-border bg-accent/5 p-2">
            {/* Global entries */}
            <div className="space-y-1">
              <button
                onClick={() => setTarget({ kind: "global-vars" })}
                className={cn(
                  "flex w-full items-center gap-1.5 rounded px-2 py-1.5 text-left text-sm hover:bg-accent/60",
                  target.kind === "global-vars" &&
                    "bg-accent font-medium text-accent-foreground ring-1 ring-inset ring-border",
                )}
              >
                <Layers className="h-3.5 w-3.5 text-muted-foreground" />
                <span className="flex-1 truncate">{t("env.globalVars")}</span>
              </button>
              <button
                onClick={() => setTarget({ kind: "global-secrets" })}
                className={cn(
                  "flex w-full items-center gap-1.5 rounded px-2 py-1.5 text-left text-sm hover:bg-accent/60",
                  target.kind === "global-secrets" &&
                    "bg-accent font-medium text-accent-foreground ring-1 ring-inset ring-border",
                )}
              >
                <KeyRound className="h-3.5 w-3.5 text-muted-foreground" />
                <span className="flex-1 truncate">
                  {t("env.globalSecrets")}
                </span>
              </button>
            </div>

            <Separator className="my-2" />

            <Button
              variant="outline"
              size="sm"
              className="mb-2 w-full gap-1.5"
              onClick={() => addEnvironment(t("env.new"))}
            >
              <Plus className="h-3.5 w-3.5" /> {t("env.new")}
            </Button>
            <div className="space-y-1">
              {environments.map((e) => (
                <div key={e.id} className="group flex items-center gap-1">
                  <button
                    onClick={() => setTarget({ kind: "env", id: e.id })}
                    className={cn(
                      "flex flex-1 items-center gap-1.5 rounded px-2 py-1.5 text-left text-sm hover:bg-accent/60",
                      target.kind === "env" &&
                        target.id === e.id &&
                        "bg-accent font-medium text-accent-foreground ring-1 ring-inset ring-border",
                    )}
                  >
                    <Globe className="h-3.5 w-3.5 text-muted-foreground" />
                    <span className="flex-1 truncate">{e.name}</span>
                    {activeEnvId === e.id && (
                      <Check className="h-3.5 w-3.5 text-emerald-400" />
                    )}
                  </button>
                  <button
                    className="opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-destructive"
                    title={t("env.delete")}
                    onClick={() => {
                      removeEnvironment(e.id);
                      if (target.kind === "env" && target.id === e.id) {
                        setTarget(
                          activeEnvId === e.id
                            ? { kind: "global-vars" }
                            : {
                                kind: "env",
                                id:
                                  environments.find((x) => x.id !== e.id)?.id ??
                                  "",
                              },
                        );
                      }
                    }}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                </div>
              ))}
            </div>
          </div>

          {/* Right: editor */}
          <div className="flex-1 min-w-0">
            <ScrollArea className="h-full pr-2">
              <div className="space-y-4 px-1.5">
                {/* Current edit target indicator */}
                {targetMeta && (
                  <div className="flex items-center gap-2.5 rounded-lg border bg-muted/50 px-3 py-2.5">
                    <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
                      {targetMeta.icon}
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm font-semibold text-foreground">
                        {targetMeta.title}
                      </div>
                      <div className="truncate text-xs text-muted-foreground">
                        {targetMeta.subtitle}
                      </div>
                    </div>
                  </div>
                )}

                {target.kind === "global-vars" && (
                  <PairEditor pairs={vars} onChange={commitVars} />
                )}

                {target.kind === "global-secrets" && (
                  <PairEditor pairs={secrets} onChange={commitSecrets} secret />
                )}

                {target.kind === "env" && !selectedEnv && (
                  <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
                    {t("env.selectHint")}
                  </div>
                )}

                {target.kind === "env" && selectedEnv && (
                  <>
                    <div className="flex items-center gap-2">
                      <Label className="shrink-0 text-xs text-muted-foreground">
                        {t("env.name")}
                      </Label>
                      <Input
                        value={name}
                        onChange={(e) => {
                          setName(e.target.value);
                          renameEnvironment(selectedEnv.id, e.target.value);
                        }}
                        className="h-8 flex-1 text-sm"
                      />
                      <Button
                        size="sm"
                        variant={
                          activeEnvId === selectedEnv.id
                            ? "secondary"
                            : "default"
                        }
                        onClick={() =>
                          setActiveEnv(
                            activeEnvId === selectedEnv.id
                              ? null
                              : selectedEnv.id,
                          )
                        }
                      >
                        {activeEnvId === selectedEnv.id
                          ? t("env.current")
                          : t("env.setCurrent")}
                      </Button>
                    </div>

                    <div>
                      <div className="mb-1.5 flex items-center gap-1.5 text-xs font-semibold text-muted-foreground">
                        <Globe className="h-3.5 w-3.5" /> {t("env.variables")}
                      </div>
                      <PairEditor pairs={vars} onChange={commitVars} />
                    </div>

                    <Separator />

                    <div>
                      <div className="mb-1.5 flex items-center gap-1.5 text-xs font-semibold text-muted-foreground">
                        <KeyRound className="h-3.5 w-3.5" /> {t("env.secrets")}
                      </div>
                      <PairEditor
                        pairs={secrets}
                        onChange={commitSecrets}
                        secret
                      />
                    </div>
                  </>
                )}
              </div>
            </ScrollArea>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function PairEditor({
  pairs,
  onChange,
  secret,
}: {
  pairs: Pair[];
  onChange: (next: Pair[]) => void;
  secret?: boolean;
}) {
  const { t } = useT();
  return (
    <div className="space-y-1.5">
      {pairs.map((p, i) => (
        <div key={i} className="flex items-center gap-1.5">
          <Input
            value={p.key}
            onChange={(e) =>
              onChange(
                pairs.map((x, j) =>
                  j === i ? { ...x, key: e.target.value } : x,
                ),
              )
            }
            placeholder="Key"
            className="h-8 flex-1 font-mono text-xs"
          />
          <Input
            value={p.value}
            type={secret ? "password" : "text"}
            onChange={(e) =>
              onChange(
                pairs.map((x, j) =>
                  j === i ? { ...x, value: e.target.value } : x,
                ),
              )
            }
            placeholder="Value"
            className="h-8 flex-1 font-mono text-xs"
          />
          <Button
            variant="ghost"
            size="icon-sm"
            className="text-destructive"
            onClick={() => onChange(pairs.filter((_, j) => j !== i))}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      ))}
      <Button
        variant="ghost"
        size="sm"
        className="text-xs"
        onClick={() => onChange([...pairs, { key: "", value: "" }])}
      >
        <Plus className="h-3.5 w-3.5" /> {t("common.addRow") as string}
      </Button>
    </div>
  );
}
