// Right-click context menu for scenario list items.
import { useEffect, useRef, useState } from "react";
import { Ellipsis } from "lucide-react";

export interface CtxMenuItem {
  label: string;
  icon: typeof Ellipsis;
  action: () => void;
  danger?: boolean;
}

export function ContextMenu({
  items,
  children,
}: {
  items: CtxMenuItem[];
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node))
        setOpen(false);
    };
    const keyHandler = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", handler);
    document.addEventListener("keydown", keyHandler);
    return () => {
      document.removeEventListener("mousedown", handler);
      document.removeEventListener("keydown", keyHandler);
    };
  }, [open]);

  return (
    <div
      ref={ref}
      className="relative"
      onContextMenu={(e) => {
        e.preventDefault();
        setPos({ x: e.clientX, y: e.clientY });
        setOpen(true);
      }}
    >
      {children}
      {open && (
        <div
          className="fixed z-50 min-w-35 rounded-md border border-border bg-popover p-1 shadow-md"
          style={{ left: pos.x, top: pos.y }}
        >
          {items.map((it, i) => {
            const Icon = it.icon;
            return (
              <button
                key={i}
                className={`flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-xs hover:bg-accent ${it.danger ? "text-destructive hover:text-destructive" : ""}`}
                onClick={() => {
                  it.action();
                  setOpen(false);
                }}
              >
                <Icon className="h-3.5 w-3.5" /> {it.label}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
