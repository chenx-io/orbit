import { useState } from "react";
import { MessageSquareHeart, Send, Star } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import {
  Dialog,
  DialogContent,
  DialogDescription,
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
import { useAppStore } from "@/store/useStore";
import { MODULES } from "@/components/layout/modules";
import { useT } from "@/lib/i18n";
import type { ModuleKey } from "@/data/types";
import { cn } from "@/lib/utils";

export function FeedbackWidget() {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [rating, setRating] = useState(5);
  const [module, setModule] = useState<ModuleKey>("api");
  const addFeedback = useAppStore((s) => s.addFeedback);
  const { t } = useT();

  const submit = () => {
    if (!text.trim()) return;
    addFeedback({ text: text.trim(), rating, module });
    setText("");
    setRating(5);
    setOpen(false);
  };

  return (
    <>
      <Button
        onClick={() => setOpen(true)}
        size="icon"
        className="fixed bottom-4 right-4 z-40 h-11 w-11 rounded-full shadow-lg"
        title={t("feedback.hint")}
      >
        <MessageSquareHeart className="h-5 w-5" />
      </Button>

      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("feedback.title")}</DialogTitle>
            <DialogDescription>{t("feedback.desc")}</DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <div>
              <div className="mb-1 text-xs text-muted-foreground">
                {t("feedback.rating")}
              </div>
              <div className="flex gap-1">
                {[1, 2, 3, 4, 5].map((n) => (
                  <button
                    key={n}
                    onClick={() => setRating(n)}
                    className="transition-transform hover:scale-110"
                  >
                    <Star
                      className={cn(
                        "h-6 w-6",
                        n <= rating
                          ? "fill-amber-400 text-amber-400"
                          : "text-muted-foreground",
                      )}
                    />
                  </button>
                ))}
              </div>
            </div>
            <div>
              <div className="mb-1 text-xs text-muted-foreground">
                {t("feedback.module")}
              </div>
              <Select
                value={module}
                onValueChange={(v) => setModule(v as ModuleKey)}
              >
                <SelectTrigger className="h-8 text-xs">
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
              placeholder={t("feedback.placeholder")}
              className="min-h-25 text-sm"
            />
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={submit}
              disabled={!text.trim()}
              className="gap-1.5"
            >
              <Send className="h-4 w-4" /> {t("feedback.submit")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
