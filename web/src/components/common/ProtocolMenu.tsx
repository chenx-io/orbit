// Protocol submenu for a new request: the folder / tree "new request" entry expands the protocol list.
import { Plus } from "lucide-react";
import {
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuItem,
} from "@/components/ui/dropdown-menu";
import {
  cn,
  PROTOCOL_LABEL,
  PROTOCOL_OPTIONS,
  protocolColor,
} from "@/lib/utils";
import type { ProtocolKind } from "@/data/types";

/** "New request" submenu: HTTP (default) plus the network protocols */
export function ProtocolNewMenu({
  label,
  onSelect,
}: {
  label: string;
  onSelect: (protocol: ProtocolKind) => void;
}) {
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        <Plus className="mr-2 h-4 w-4" />
        {label}
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent>
        {PROTOCOL_OPTIONS.map((p) => (
          <DropdownMenuItem key={p} onClick={() => onSelect(p)}>
            <span
              className={cn(
                "mr-2 inline-block w-11 text-right font-mono text-xs font-semibold",
                protocolColor(p),
              )}
            >
              {p === "http" ? "HTTP" : (PROTOCOL_LABEL[p] ?? p)}
            </span>
            <span className="capitalize">{p}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  );
}
