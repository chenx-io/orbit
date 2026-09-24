// Name display: ellipsis when overlong plus the full content on hover (via the native title).
// The native title delay is system-controlled (about 1s) but its position is guaranteed by the browser, making it reliable everywhere.
import { cn } from "@/lib/utils";

export function NameTooltip({
  name,
  className,
}: {
  name: string;
  className?: string;
}) {
  return (
    <span title={name} className={cn("min-w-0 truncate", className)}>
      {name}
    </span>
  );
}
