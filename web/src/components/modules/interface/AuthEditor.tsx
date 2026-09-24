// Auth editor: renders the config form for the selected auth.type (none/bearer/basic/apikey/oauth2).
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { DynamicValuePicker } from "@/components/common/DynamicValuePicker";
import { useT } from "@/lib/i18n";
import type { AuthType, HttpRequest, Locale } from "@/data/types";

export function AuthEditor({
  req,
  set,
  locale,
}: {
  req: HttpRequest;
  set: (p: Partial<HttpRequest>) => void;
  locale: Locale;
}) {
  const auth = req.auth;
  const { t } = useT();
  const setAuth = (patch: Partial<typeof auth>) =>
    set({ auth: { ...auth, ...patch } });
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <Label className="w-16 text-xs">{t("auth.type")}</Label>
        <Select
          value={auth.type}
          onValueChange={(v) => setAuth({ type: v as AuthType })}
        >
          <SelectTrigger className="h-8 w-48">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="none">{t("auth.none")}</SelectItem>
            <SelectItem value="bearer">{t("auth.bearer")}</SelectItem>
            <SelectItem value="basic">{t("auth.basic")}</SelectItem>
            <SelectItem value="apikey">{t("auth.apikey")}</SelectItem>
            <SelectItem value="oauth2">{t("auth.oauth2")}</SelectItem>
          </SelectContent>
        </Select>
      </div>
      {auth.type === "bearer" && (
        <div className="flex items-center gap-2">
          <Label className="w-16 text-xs">{t("auth.token")}</Label>
          <Input
            value={auth.token ?? ""}
            onChange={(e) => setAuth({ token: e.target.value })}
            className="h-8 flex-1 font-mono text-xs"
            placeholder="{{token}}"
          />
          <DynamicValuePicker
            onInsert={(t) => setAuth({ token: `{{${t}}}` })}
          />
        </div>
      )}
      {auth.type === "basic" && (
        <>
          <div className="flex items-center gap-2">
            <Label className="w-16 text-xs">{t("auth.username")}</Label>
            <Input
              value={auth.username ?? ""}
              onChange={(e) => setAuth({ username: e.target.value })}
              className="h-8 flex-1 text-xs"
            />
          </div>
          <div className="flex items-center gap-2">
            <Label className="w-16 text-xs">{t("auth.password")}</Label>
            <Input
              type="password"
              value={auth.password ?? ""}
              onChange={(e) => setAuth({ password: e.target.value })}
              className="h-8 flex-1 text-xs"
            />
          </div>
        </>
      )}
      {auth.type === "apikey" && (
        <>
          <div className="flex items-center gap-2">
            <Label className="w-16 text-xs">{t("auth.key")}</Label>
            <Input
              value={auth.key ?? ""}
              onChange={(e) => setAuth({ key: e.target.value })}
              className="h-8 flex-1 text-xs"
            />
          </div>
          <div className="flex items-center gap-2">
            <Label className="w-16 text-xs">{t("auth.value")}</Label>
            <Input
              value={auth.value ?? ""}
              onChange={(e) => setAuth({ value: e.target.value })}
              className="h-8 flex-1 font-mono text-xs"
              placeholder="{{api_key}}"
            />
          </div>
          <div className="flex items-center gap-2">
            <Label className="w-16 text-xs">{t("auth.location")}</Label>
            <Select
              value={auth.addTo ?? "header"}
              onValueChange={(v) => setAuth({ addTo: v as "header" | "query" })}
            >
              <SelectTrigger className="h-8 w-48">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="header">{t("auth.header")}</SelectItem>
                <SelectItem value="query">{t("auth.query")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </>
      )}
      {auth.type === "oauth2" && (
        <div className="text-xs text-muted-foreground">
          OAuth 2.0 —{" "}
          {locale === "zh-CN"
            ? "在 Rust 引擎中完成 token 获取与刷新（演示）"
            : "Token acquisition & refresh in Rust engine (demo)"}
        </div>
      )}
    </div>
  );
}
