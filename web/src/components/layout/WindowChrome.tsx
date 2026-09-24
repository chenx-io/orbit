// Window chrome (Tauri decorations:false):
// - WindowControls: custom minimize / maximize / close (not rendered on the web)
// - WindowResizeEdges: edge / corner resize dragging for a frameless window (not rendered on the web)
import { useEffect, useState } from "react";
import { useT } from "@/lib/i18n";
import { Minus, Square, Copy, X } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauri } from "@/lib/bridge";

type ResizeDir =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";

function win() {
  return getCurrentWindow();
}

/** Custom window control buttons (Tauri decorations:false; not rendered on the web) */
export function WindowControls() {
  const { t } = useT();
  const [isMax, setIsMax] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    void win()
      .isMaximized()
      .then(setIsMax)
      .catch(() => {});
    void win()
      .onResized(() => {
        void win()
          .isMaximized()
          .then(setIsMax)
          .catch(() => {});
      })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
    };
  }, []);

  if (!isTauri()) return null;

  const ctl =
    "flex h-full w-11 items-center justify-center text-muted-foreground transition-colors hover:bg-accent/15 hover:text-foreground";
  const fail = (e: unknown) =>
    console.warn("[window] window operation failed", e);

  return (
    <div
      data-tauri-drag-region="false"
      className="ml-1 flex h-full shrink-0 items-stretch"
    >
      <button
        className={ctl}
        title={t("window.minimize")}
        onClick={() => void win().minimize().catch(fail)}
      >
        <Minus className="h-4 w-4" />
      </button>
      <button
        className={ctl}
        title={isMax ? t("window.restore") : t("window.maximize")}
        onClick={() => void win().toggleMaximize().catch(fail)}
      >
        {isMax ? (
          <Copy className="h-3.5 w-3.5" />
        ) : (
          <Square className="h-3.5 w-3.5" />
        )}
      </button>
      <button
        className={ctl + " hover:bg-red-500 hover:text-white"}
        title={t("window.close")}
        onClick={() => void win().close().catch(fail)}
      >
        <X className="h-4 w-4" />
      </button>
    </div>
  );
}

/** Frameless window: edge / corner resize handles in 8 directions (the cursor changes on hover and pressing starts Tauri's native resize drag) */
export function WindowResizeEdges() {
  if (!isTauri()) return null;

  const start = (dir: ResizeDir) => (e: React.PointerEvent) => {
    e.preventDefault();
    try {
      void win()
        .startResizeDragging(dir)
        .catch(() => {});
    } catch {
      /* Resizing is not allowed while maximized, so ignore */
    }
  };

  const edge = "absolute pointer-events-auto";
  const corner = "absolute pointer-events-auto z-50";
  const thickness = 5;

  return (
    <div className="pointer-events-none fixed inset-0 z-40">
      {/* Edges */}
      <div
        className={`${edge} left-0 top-0 h-full cursor-ew-resize`}
        style={{ width: thickness }}
        onPointerDown={start("West")}
      />
      <div
        className={`${edge} right-0 top-0 h-full cursor-ew-resize`}
        style={{ width: thickness }}
        onPointerDown={start("East")}
      />
      <div
        className={`${edge} left-0 top-0 w-full cursor-ns-resize`}
        style={{ height: thickness }}
        onPointerDown={start("North")}
      />
      <div
        className={`${edge} bottom-0 left-0 w-full cursor-ns-resize`}
        style={{ height: thickness }}
        onPointerDown={start("South")}
      />
      {/* Corners */}
      <div
        className={`${corner} left-0 top-0 cursor-nwse-resize`}
        style={{ width: 10, height: 10 }}
        onPointerDown={start("NorthWest")}
      />
      <div
        className={`${corner} right-0 top-0 cursor-nesw-resize`}
        style={{ width: 10, height: 10 }}
        onPointerDown={start("NorthEast")}
      />
      <div
        className={`${corner} bottom-0 left-0 cursor-nesw-resize`}
        style={{ width: 10, height: 10 }}
        onPointerDown={start("SouthWest")}
      />
      <div
        className={`${corner} bottom-0 right-0 cursor-nwse-resize`}
        style={{ width: 10, height: 10 }}
        onPointerDown={start("SouthEast")}
      />
    </div>
  );
}
