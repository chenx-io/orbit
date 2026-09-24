// Parse result lists: the grouped endpoint list (EndpointGroups) and the model detail table (SchemaItem).
import { useMemo, useState } from "react";
import {
  CheckSquare,
  ChevronDown,
  ChevronRight,
  FolderOpen,
  Square,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { cn, methodBg } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import type { ImportedEndpoint, ImportedSchema } from "@/lib/bridge";

// ─── Grouped endpoint list ────────────────────────────────────

export function EndpointGroups({
  endpoints,
  selected,
  onToggle,
}: {
  endpoints: ImportedEndpoint[];
  selected: Set<number>;
  onToggle: (i: number) => void;
}) {
  const groups = useMemo(() => {
    const map = new Map<
      string,
      { endpoints: ImportedEndpoint[]; indices: number[] }
    >();
    for (let i = 0; i < endpoints.length; i++) {
      const g = endpoints[i].group || "";
      if (!map.has(g)) map.set(g, { endpoints: [], indices: [] });
      map.get(g)!.endpoints.push(endpoints[i]);
      map.get(g)!.indices.push(i);
    }
    return Array.from(map.entries()).sort(([a], [b]) => a.localeCompare(b));
  }, [endpoints]);

  return (
    <>
      {groups.map(([group, data]) => (
        <EndpointGroup
          key={group}
          groupName={group}
          endpoints={data.endpoints}
          indices={data.indices}
          selected={selected}
          onToggle={onToggle}
        />
      ))}
    </>
  );
}

function EndpointGroup({
  groupName,
  endpoints,
  indices,
  selected,
  onToggle,
}: {
  groupName: string;
  endpoints: ImportedEndpoint[];
  indices: number[];
  selected: Set<number>;
  onToggle: (i: number) => void;
}) {
  const [open, setOpen] = useState(true);
  const allSelected = indices.every((i) => selected.has(i));
  const someSelected = indices.some((i) => selected.has(i));
  const childSelected = indices.filter((i) => selected.has(i)).length;

  const toggleGroup = () => {
    if (allSelected) {
      indices.forEach((i) => {
        if (selected.has(i)) onToggle(i);
      });
    } else {
      indices.filter((i) => !selected.has(i)).forEach((i) => onToggle(i));
    }
  };

  return (
    <div>
      <div
        className="flex items-center gap-1.5 rounded px-2 py-1 text-xs hover:bg-accent/10 cursor-pointer"
        onClick={() => setOpen(!open)}
      >
        {/* Expand / collapse icon */}
        {open ? (
          <ChevronDown className="h-3 w-3 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground" />
        )}
        {/* Group checkbox */}
        <div
          className="flex h-5 w-5 shrink-0 items-center justify-center"
          onClick={(e) => {
            e.stopPropagation();
            toggleGroup();
          }}
        >
          {allSelected ? (
            <CheckSquare className="h-4 w-4 text-primary cursor-pointer" />
          ) : someSelected ? (
            <div className="flex h-4 w-4 items-center justify-center rounded border-2 border-primary bg-primary/20 cursor-pointer">
              <div className="h-2 w-0.5 bg-primary" />
            </div>
          ) : (
            <Square className="h-4 w-4 text-muted-foreground cursor-pointer" />
          )}
        </div>
        <FolderOpen className="h-3.5 w-3.5 text-amber-400 shrink-0" />
        <span className="flex-1 text-left font-semibold text-muted-foreground truncate">
          {groupName || "(ungrouped)"}
        </span>
        <Badge variant="outline" className="text-xs shrink-0">
          {childSelected}/{endpoints.length}
        </Badge>
      </div>
      {open &&
        endpoints.map((ep, j) => {
          const idx = indices[j];
          return (
            <EndpointItem
              key={idx}
              ep={ep}
              selected={selected.has(idx)}
              onToggle={() => onToggle(idx)}
            />
          );
        })}
    </div>
  );
}

function EndpointItem({
  ep,
  selected,
  onToggle,
}: {
  ep: ImportedEndpoint;
  selected: boolean;
  onToggle: () => void;
}) {
  return (
    <div
      className="ml-5 flex items-center gap-2 rounded py-1 pr-1.5 text-xs hover:bg-accent/10 cursor-pointer"
      onClick={onToggle}
    >
      <div
        className="flex h-4 w-4 shrink-0 items-center justify-center"
        onClick={(e) => e.stopPropagation()}
      >
        {selected ? (
          <CheckSquare className="h-3.5 w-3.5 text-primary cursor-pointer" />
        ) : (
          <Square className="h-3.5 w-3.5 text-muted-foreground cursor-pointer" />
        )}
      </div>
      <Badge
        variant="outline"
        className={cn("font-mono text-xs shrink-0", methodBg(ep.method))}
      >
        {ep.method}
      </Badge>
      <span className="min-w-0 flex-1 truncate font-medium">{ep.name}</span>
      <span className="shrink-0 truncate font-mono text-xs text-muted-foreground max-w-70">
        {ep.url}
      </span>
    </div>
  );
}

// ─── Schema detail table ───────────────────────────────────

export function SchemaItem({
  schema,
  selected,
  onToggle,
}: {
  schema: ImportedSchema;
  selected: boolean;
  onToggle: () => void;
}) {
  const [open, setOpen] = useState(false);
  const { t } = useT();
  const s = schema.schema_json ?? {};
  const props: Record<string, any> = s.properties ?? {};
  const required: string[] = s.required ?? [];
  const entries = Object.entries(props);

  return (
    <div>
      <div
        className="flex items-center gap-1.5 rounded py-1 pr-1 text-xs hover:bg-accent/10 cursor-pointer"
        onClick={() => setOpen(!open)}
      >
        {open ? (
          <ChevronDown className="h-3 w-3 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground" />
        )}
        <div
          className="flex h-5 w-5 shrink-0 items-center justify-center"
          onClick={(e) => {
            e.stopPropagation();
            onToggle();
          }}
        >
          {selected ? (
            <CheckSquare className="h-4 w-4 text-primary" />
          ) : (
            <Square className="h-4 w-4 text-muted-foreground" />
          )}
        </div>
        <span className="min-w-0 flex-1 truncate text-left font-mono font-medium">
          {schema.name}
        </span>
        <span className="text-xs text-muted-foreground">
          {s.type ?? "object"}
        </span>
      </div>
      {open && (
        <div className="ml-10 mr-1 mb-1 overflow-auto rounded border border-border">
          {entries.length > 0 ? (
            <table className="w-full text-xs">
              <thead>
                <tr className="border-b border-border bg-muted/30 text-muted-foreground">
                  <th className="px-2 py-1.5 text-left font-medium">
                    {t("import.fieldName")}
                  </th>
                  <th className="px-2 py-1.5 text-left font-medium">
                    {t("import.fieldType")}
                  </th>
                  <th className="px-2 py-1.5 text-center font-medium w-12">
                    {t("import.fieldRequired")}
                  </th>
                  <th className="px-2 py-1.5 text-left font-medium">
                    {t("import.fieldExample")}
                  </th>
                  <th className="px-2 py-1.5 text-left font-medium">
                    {t("import.fieldDesc")}
                  </th>
                </tr>
              </thead>
              <tbody>
                {entries.map(([name, field]: [string, any]) => {
                  const isReq = required.includes(name);
                  const type = field.type ?? "string";
                  const example = field.example ?? field.default ?? "";
                  const desc = field.description ?? "";
                  return (
                    <tr
                      key={name}
                      className="border-b border-border/50 last:border-0 hover:bg-accent/5"
                    >
                      <td className="px-2 py-1 font-mono font-medium">
                        {name}
                      </td>
                      <td className="px-2 py-1">
                        <span className="rounded bg-muted px-1 py-px font-mono text-muted-foreground">
                          {type}
                        </span>
                      </td>
                      <td className="px-2 py-1 text-center">
                        {isReq ? (
                          <span className="text-rose-400 font-bold">●</span>
                        ) : (
                          <span className="text-muted-foreground">—</span>
                        )}
                      </td>
                      <td
                        className="px-2 py-1 font-mono text-muted-foreground max-w-30 truncate"
                        title={String(example)}
                      >
                        {example ? String(example) : "—"}
                      </td>
                      <td
                        className="px-2 py-1 text-muted-foreground max-w-40 truncate"
                        title={desc}
                      >
                        {desc || "—"}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          ) : (
            <pre className="p-2 font-mono text-xs leading-relaxed max-h-50 overflow-auto">
              {JSON.stringify(schema.schema_json, null, 2)}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
