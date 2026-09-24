// Package editor area: package name + raw proto content + Auth + pre/post script.
import { useState } from "react";
import { FileCode2, Package as PackageIcon, Save } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import { AuthEditor } from "./AuthEditor";
import { ScriptEditor } from "./ScriptEditor";

export function GrpcPackageEditor({
  collectionId,
  id,
}: {
  collectionId: string;
  id: string;
}) {
  const { t } = useT();
  const locale = useAppStore((s) => s.locale);
  const collections = useAppStore((s) => s.collections);
  const updateProto = useAppStore((s) => s.updateGrpcPackageProto);
  const updateConfig = useAppStore((s) => s.updateGrpcPackageConfig);

  // id looks like `pkg-${collectionId}-${packageName}`
  const prefix = `pkg-${collectionId}-`;
  const packageName = id.startsWith(prefix) ? id.slice(prefix.length) : id;
  const col = collections.find((c) => c.id === collectionId);
  const pkg = col?.grpc?.packages.find((p) => p.name === packageName);

  const [protoDraft, setProtoDraft] = useState<string | null>(null);
  const [tab, setTab] = useState<"proto" | "auth" | "prescript" | "postscript">(
    "proto",
  );

  if (!pkg) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-muted-foreground">
        {t("grpc.packageNotFound")}
      </div>
    );
  }

  const protoValue = protoDraft ?? pkg.proto ?? "";
  const hasProtoDirty = protoDraft !== null && protoDraft !== (pkg.proto ?? "");

  // AuthEditor expects an HttpRequest shape; here we only forward the auth field
  const authReq = {
    id: `pkg-${packageName}`,
    name: packageName,
    protocol: "http",
    method: "GET",
    url: "",
    headers: [],
    queryParams: [],
    body: "",
    bodyMode: "none",
    contentType: "",
    formParams: [],
    binaryFile: null,
    auth: pkg.auth ?? { type: "none" },
    cookies: [],
  } as const;

  return (
    <div className="flex h-full flex-col overflow-auto">
      <div className="mb-4 flex items-center gap-2 border-b border-border px-4 py-3">
        <PackageIcon className="h-5 w-5 text-fuchsia-400" />
        <h2 className="text-sm font-semibold">{packageName || "(default)"}</h2>
        <Badge variant="outline" className="text-xs">
          package
        </Badge>
        <div className="flex-1" />
        {col?.grpc?.source.type === "proto" ? (
          <span className="text-xs text-muted-foreground">
            {t("grpc.sourceProto")}
          </span>
        ) : (
          <span className="text-xs text-muted-foreground">
            {t("grpc.sourceReflect")}
            {col?.grpc?.source.type === "reflection"
              ? ` · ${col.grpc.source.target}`
              : ""}
          </span>
        )}
      </div>

      <div className="flex-1 px-4">
        <Tabs value={tab} onValueChange={(v) => setTab(v as typeof tab)}>
          <TabsList>
            <TabsTrigger value="proto">
              <FileCode2 className="mr-1.5 h-3.5 w-3.5" />
              {t("grpc.protoContent")}
            </TabsTrigger>
            <TabsTrigger value="auth">Auth</TabsTrigger>
            <TabsTrigger value="prescript">{t("grpc.preScript")}</TabsTrigger>
            <TabsTrigger value="postscript">{t("grpc.postScript")}</TabsTrigger>
          </TabsList>

          <TabsContent value="proto" className="mt-3">
            {col?.grpc?.source.type === "reflection" ? (
              <div className="rounded-md border border-border bg-muted/20 p-3 text-xs text-muted-foreground">
                {t("grpc.reflectNoProto")}
              </div>
            ) : (
              <>
                <Textarea
                  value={protoValue}
                  onChange={(e) => setProtoDraft(e.target.value)}
                  className="min-h-64 font-mono text-xs"
                  placeholder={t("grpc.protoPlaceholder")}
                  spellCheck={false}
                />
                <div className="mt-2 flex items-center gap-2">
                  <Button
                    size="sm"
                    disabled={!hasProtoDirty}
                    onClick={() => {
                      updateProto(collectionId, packageName, protoDraft ?? "");
                      setProtoDraft(null);
                    }}
                  >
                    <Save className="mr-1.5 h-3.5 w-3.5" />
                    {t("grpc.saveProto")}
                  </Button>
                  {hasProtoDirty && (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setProtoDraft(null)}
                    >
                      {t("grpc.revert")}
                    </Button>
                  )}
                </div>
              </>
            )}
          </TabsContent>

          <TabsContent value="auth" className="mt-3">
            <AuthEditor
              req={authReq as any}
              set={(patch) =>
                updateConfig(collectionId, packageName, { auth: patch.auth })
              }
              locale={locale}
            />
          </TabsContent>

          <TabsContent value="prescript" className="mt-3 space-y-2">
            <div className="mb-1 text-xs text-muted-foreground">
              {t("grpc.preScriptHint")}
            </div>
            <ScriptEditor
              value={pkg.prereqScript ?? ""}
              onChange={(v) =>
                updateConfig(collectionId, packageName, { prereqScript: v })
              }
              kind="pre"
              placeholder={t("grpc.preScriptPlaceholder")}
            />
          </TabsContent>
          <TabsContent value="postscript" className="mt-3 space-y-2">
            <div className="mb-1 text-xs text-muted-foreground">
              {t("grpc.postScriptHint")}
            </div>
            <ScriptEditor
              value={pkg.postreqScript ?? ""}
              onChange={(v) =>
                updateConfig(collectionId, packageName, { postreqScript: v })
              }
              kind="post"
              placeholder={t("grpc.postScriptPlaceholder")}
            />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}
