// Import target folder picker: a two-level dropdown (collection + folder).
import { ChevronDown, FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useT } from "@/lib/i18n";
import type { CollectionItem } from "@/data/types";

/** Resolve the folder path for a target id; null means the target is the root. */
function getFolderPath(
  items: CollectionItem[],
  targetId: string | null,
): string | null {
  if (!targetId) return null;
  for (const it of items) {
    if (it.id === targetId && it.type === "folder") return it.name;
    if (it.type === "folder") {
      const found = getFolderPath(it.items, targetId);
      if (found) return found;
    }
  }
  return null;
}

function flatFolders(
  items: CollectionItem[],
  depth: number,
): { id: string; name: string; depth: number }[] {
  const out: { id: string; name: string; depth: number }[] = [];
  for (const it of items) {
    if (it.type === "folder") {
      out.push({ id: it.id, name: it.name, depth });
      out.push(...flatFolders(it.items, depth + 1));
    }
  }
  return out;
}

export function CollectionFolderPicker({
  collections,
  selectedColId,
  selectedFolderId,
  onSelect,
}: {
  collections: { id: string; name: string; items: CollectionItem[] }[];
  selectedColId: string;
  selectedFolderId: string | null;
  onSelect: (colId: string, folderId: string | null) => void;
}) {
  const { t } = useT();
  const activeCol = collections.find((c) => c.id === selectedColId);
  const activeName = activeCol?.name ?? "—";

  return (
    <div className="flex items-center gap-1.5">
      {/* Collection dropdown */}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="sm" className="h-7 gap-1 text-xs">
            <span className="max-w-30 truncate">{activeName}</span>
            <ChevronDown className="h-3 w-3 text-muted-foreground" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          {collections.map((col) => (
            <DropdownMenuItem
              key={col.id}
              onClick={() => onSelect(col.id, null)}
              className="text-xs"
            >
              {col.name}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
      {/* Folder dropdown */}
      {activeCol && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="sm" className="h-7 gap-1 text-xs">
              <FolderOpen className="h-3 w-3 text-amber-400" />
              <span className="max-w-35 truncate text-muted-foreground">
                {getFolderPath(activeCol.items, selectedFolderId) ??
                  t("import.rootDir")}
              </span>
              <ChevronDown className="h-3 w-3 text-muted-foreground" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="max-h-55 overflow-auto">
            <DropdownMenuItem
              onClick={() => onSelect(selectedColId, null)}
              className="text-xs pl-2"
            >
              {t("import.rootDir")}
            </DropdownMenuItem>
            {flatFolders(activeCol.items, 1).map((f) => (
              <DropdownMenuItem
                key={f.id}
                onClick={() => onSelect(selectedColId, f.id)}
                className="text-xs"
                style={{ paddingLeft: 8 + f.depth * 12 }}
              >
                {f.name}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </div>
  );
}
