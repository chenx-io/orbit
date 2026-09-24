import { Settings2 } from "lucide-react";
import { useAppStore } from "@/store/useStore";
import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

export function EnvSwitcher() {
  const environments = useAppStore((s) => s.environments);
  const activeEnvId = useAppStore((s) => s.activeEnvId);
  const setActiveEnv = useAppStore((s) => s.setActiveEnv);
  const setEnvEditorOpen = useAppStore((s) => s.setEnvEditorOpen);
  const { t } = useT();

  return (
    <div className="flex items-center gap-1">
      <Select value={activeEnvId ?? ""} onValueChange={(v) => setActiveEnv(v)}>
        <SelectTrigger className="h-6 w-30 text-xs">
          <SelectValue placeholder={t("env.selectPlaceholder") as string} />
        </SelectTrigger>
        <SelectContent>
          {environments.map((e) => (
            <SelectItem key={e.id} value={e.id} className="text-xs">
              {e.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Button
        variant="ghost"
        size="icon-sm"
        className="h-6 w-6"
        title={t("env.manage")}
        onClick={() => setEnvEditorOpen(true)}
      >
        <Settings2 className="h-3.5 w-3.5" />
      </Button>
    </div>
  );
}
