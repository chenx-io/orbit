import { useMemo, useState, type ComponentType } from "react";
import {
  BarChart3,
  Activity,
  Send,
  Gauge,
  Workflow,
  Boxes,
  Download,
  MessageSquareHeart,
  Star,
  Trash2,
} from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn, timeAgo } from "@/lib/utils";
import { useAppStore } from "@/store/useStore";
import { MODULES } from "@/components/layout/modules";
import { useT, type TKey } from "@/lib/i18n";
import type { AnalyticsEvent, ModuleKey } from "@/data/types";

const EVENT_META: {
  key: AnalyticsEvent;
  labelKey: string;
  icon: ComponentType<{ className?: string }>;
  color: string;
}[] = [
  {
    key: "module_view",
    labelKey: "analytics.event.module_view",
    icon: Activity,
    color: "text-blue-400",
  },
  {
    key: "request_send",
    labelKey: "analytics.event.request_send",
    icon: Send,
    color: "text-emerald-400",
  },
  {
    key: "load_start",
    labelKey: "analytics.event.load_start",
    icon: Gauge,
    color: "text-amber-400",
  },
  {
    key: "load_stop",
    labelKey: "analytics.event.load_stop",
    icon: Gauge,
    color: "text-orange-400",
  },
  {
    key: "scenario_run",
    labelKey: "analytics.event.scenario_run",
    icon: Workflow,
    color: "text-violet-400",
  },
  {
    key: "plugin_install",
    labelKey: "analytics.event.plugin_install",
    icon: Boxes,
    color: "text-cyan-400",
  },
  {
    key: "import",
    labelKey: "analytics.event.import",
    icon: Download,
    color: "text-indigo-400",
  },
  {
    key: "feedback",
    labelKey: "analytics.event.feedback",
    icon: MessageSquareHeart,
    color: "text-rose-400",
  },
];

export function AnalyticsModule() {
  const analytics = useAppStore((s) => s.analytics);
  const feedback = useAppStore((s) => s.feedback);
  const addFeedback = useAppStore((s) => s.addFeedback);
  const clearHistory = useAppStore((s) => s.clearHistory);

  const [text, setText] = useState("");
  const [rating, setRating] = useState(5);
  const [module, setModule] = useState<ModuleKey>("api");
  const { t, format } = useT();

  const maxCount = useMemo(
    () => Math.max(1, ...Object.values(analytics)),
    [analytics],
  );
  const totalEvents = useMemo(
    () => Object.values(analytics).reduce((a, b) => a + b, 0),
    [analytics],
  );

  const submit = () => {
    if (!text.trim()) return;
    addFeedback({ text: text.trim(), rating, module });
    setText("");
    setRating(5);
  };

  return (
    <div className="flex h-full">
      {/* Left: event metrics */}
      <div className="w-85 shrink-0 overflow-auto border-r border-border bg-card/30 p-4">
        <div className="mb-3 flex items-center gap-2">
          <BarChart3 className="h-4 w-4 text-primary" />
          <h2 className="text-sm font-semibold">{t("analytics.title")}</h2>
          <Badge variant="outline" className="ml-auto">
            {format("analytics.total", totalEvents)}
          </Badge>
        </div>
        <div className="space-y-2.5">
          {EVENT_META.map((e) => {
            const count = analytics[e.key];
            const pct = (count / maxCount) * 100;
            const Icon = e.icon;
            return (
              <div key={e.key}>
                <div className="mb-1 flex items-center justify-between text-xs">
                  <span className="flex items-center gap-1.5">
                    <Icon className={cn("h-3.5 w-3.5", e.color)} />
                    {t(e.labelKey as TKey)}
                  </span>
                  <span className="font-mono font-bold">{count}</span>
                </div>
                <div className="h-2 overflow-hidden rounded bg-secondary">
                  <div
                    className={cn(
                      "h-full rounded transition-all",
                      e.color.replace("text-", "bg-"),
                    )}
                    style={{ width: `${pct}%` }}
                  />
                </div>
              </div>
            );
          })}
        </div>

        <div className="mt-4 rounded-lg border border-border bg-accent/5 p-3 text-xs leading-relaxed text-muted-foreground">
          {t("analytics.desc")}
        </div>
      </div>

      {/* Right: feedback */}
      <div className="flex-1 overflow-auto p-4">
        <div className="mb-3 flex items-center gap-2">
          <MessageSquareHeart className="h-4 w-4 text-rose-400" />
          <h2 className="text-sm font-semibold">
            {t("analytics.feedbackTitle")}
          </h2>
          <Badge variant="outline" className="ml-auto">
            {feedback.length}
          </Badge>
        </div>

        <Card className="mb-3">
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">
              {t("analytics.feedbackCard")}
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            <div className="flex items-center gap-3">
              <div className="flex gap-0.5">
                {[1, 2, 3, 4, 5].map((n) => (
                  <button
                    key={n}
                    onClick={() => setRating(n)}
                    className="hover:scale-110 transition-transform"
                  >
                    <Star
                      className={cn(
                        "h-5 w-5",
                        n <= rating
                          ? "fill-amber-400 text-amber-400"
                          : "text-muted-foreground",
                      )}
                    />
                  </button>
                ))}
              </div>
              <Select
                value={module}
                onValueChange={(v) => setModule(v as ModuleKey)}
              >
                <SelectTrigger className="h-8 w-40 text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {MODULES.map((m) => (
                    <SelectItem key={m.key} value={m.key}>
                      {t(m.i18nLabel as any) as string}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <Textarea
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder={t("analytics.feedbackPlaceholder")}
              className="min-h-18 text-sm"
            />
            <div className="flex justify-end">
              <Button size="sm" onClick={submit} disabled={!text.trim()}>
                {t("analytics.feedbackSubmit")}
              </Button>
            </div>
          </CardContent>
        </Card>

        <ScrollArea className="h-[calc(100%-9rem)]">
          {feedback.length === 0 ? (
            <div className="p-8 text-center text-xs text-muted-foreground">
              {t("analytics.feedbackEmpty")}
            </div>
          ) : (
            <div className="space-y-2">
              {feedback.map((f) => {
                const mod = MODULES.find((m) => m.key === f.module);
                return (
                  <div
                    key={f.id}
                    className="rounded-lg border border-border p-3"
                  >
                    <div className="mb-1 flex items-center gap-2">
                      <span className="flex">
                        {[1, 2, 3, 4, 5].map((n) => (
                          <Star
                            key={n}
                            className={cn(
                              "h-3.5 w-3.5",
                              n <= f.rating
                                ? "fill-amber-400 text-amber-400"
                                : "text-muted-foreground/40",
                            )}
                          />
                        ))}
                      </span>
                      <Badge variant="secondary" className="text-xs">
                        {mod ? (t(mod.i18nLabel as any) as string) : f.module}
                      </Badge>
                      <span className="ml-auto text-xs text-muted-foreground">
                        {timeAgo(f.createdAt)}
                      </span>
                    </div>
                    <p className="text-sm">{f.text}</p>
                  </div>
                );
              })}
            </div>
          )}
        </ScrollArea>

        {feedback.length > 0 && (
          <div className="mt-2 flex justify-end">
            <Button
              variant="ghost"
              size="sm"
              className="text-destructive"
              onClick={clearHistory}
              title={t("analytics.clearTitle")}
            >
              <Trash2 className="h-3.5 w-3.5" /> {t("analytics.clear")}
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}
