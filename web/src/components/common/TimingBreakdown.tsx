import {
  ArrowDownToLine,
  Globe,
  Lock,
  Network,
  Timer,
  Zap,
} from "lucide-react";
import { useT } from "@/lib/i18n";

export interface StageTiming {
  dns: number;
  connect: number;
  tls: number;
  ttfb: number;
  download: number;
}

const STAGES: {
  key: keyof StageTiming;
  tKey: string;
  color: string;
  icon: typeof Globe;
}[] = [
  { key: "dns", tKey: "response.timing.dns", color: "#38bdf8", icon: Globe },
  {
    key: "connect",
    tKey: "response.timing.connect",
    color: "#a78bfa",
    icon: Network,
  },
  { key: "tls", tKey: "response.timing.tls", color: "#f472b6", icon: Lock },
  { key: "ttfb", tKey: "response.timing.ttfb", color: "#fbbf24", icon: Zap },
  {
    key: "download",
    tKey: "response.timing.download",
    color: "#34d399",
    icon: ArrowDownToLine,
  },
];

export function TimingBreakdown({
  timing,
  totalMs,
}: {
  timing?: StageTiming;
  totalMs?: number;
}) {
  const { t } = useT();

  if (!timing) {
    return (
      <div className="py-1 text-xs text-muted-foreground">
        {t("response.timingNone") as string}
      </div>
    );
  }

  const values = STAGES.map((s) => timing[s.key]);
  const sum = values.reduce((a, b) => a + b, 0) || 1;
  const total = totalMs && totalMs > 0 ? totalMs : sum;

  return (
    <div className="space-y-2 py-1">
      <div className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
        <Timer className="h-3.5 w-3.5" />
        {t("response.timing") as string}
      </div>

      {STAGES.map((s) => {
        const v = timing[s.key];
        const Icon = s.icon;
        const pct = total > 0 ? Math.round((v / total) * 100) : 0;
        return (
          <div key={s.key} className="flex items-center gap-2 text-xs">
            <span
              className="inline-block h-2.5 w-2.5 shrink-0 rounded-sm"
              style={{ backgroundColor: v > 0 ? s.color : "#52525b" }}
            />
            <Icon className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
            <span className="w-20 shrink-0 text-foreground/80">
              {t(s.tKey as any) as string}
            </span>
            <span className="font-mono tabular-nums text-foreground">
              {v > 0 ? `${v}ms` : "—"}
            </span>
            <span className="font-mono text-xs text-muted-foreground">
              {v > 0 ? `(${pct}%)` : ""}
            </span>
          </div>
        );
      })}

      <div className="flex items-center justify-between border-t border-border pt-1.5 text-xs text-muted-foreground">
        <span>
          {t("response.timing.sum") as string} {sum}ms
        </span>
        {totalMs != null && (
          <span>
            {t("response.timing.total") as string} {totalMs}ms
          </span>
        )}
      </div>
    </div>
  );
}
