import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useT } from "@/lib/i18n";

export interface SeriesDef {
  key: string;
  color: string;
  label: string;
  /** Value formatter; shows values as-is by default */
  format?: (v: number) => string;
}

interface Props {
  data: any[];
  series: SeriesDef[];
  height?: number;
  xKey?: string;
}

interface HoverInfo {
  x: number;
  index: number;
  values: { key: string; label: string; value: number; color: string }[];
}

interface TimeRange {
  start: number;
  end: number;
}

/** Maximum points drawn in one viewport; beyond that it samples by stride (zoom restores the detail) */
const MAX_DRAW_POINTS = 4000;
/** Minimum visible time span (seconds), preventing infinite zoom-in */
const MIN_SPAN = 0.1;

export function LiveChart({
  data,
  series,
  height = 220,
  xKey = "time",
}: Props) {
  const { t, locale } = useT();
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [hover, setHover] = useState<HoverInfo | null>(null);
  /** null = fit all history automatically; otherwise the visible time window [start, end] (seconds) */
  const [view, setView] = useState<TimeRange | null>(null);
  /** Drag-selection box (canvas pixel coordinates) */
  const [dragSel, setDragSel] = useState<{ x0: number; x1: number } | null>(
    null,
  );
  const dragStartRef = useRef<{ x: number } | null>(null);

  const bottomPad = 20;

  function computeTicks(
    dataMin: number,
    dataMax: number,
    maxTicks = 6,
  ): number[] {
    const range = dataMax - dataMin;
    if (range <= 0) return [dataMin];
    const rough = range / maxTicks;
    const magnitude = Math.pow(10, Math.floor(Math.log10(rough)));
    const residual = rough / magnitude;
    let step: number;
    if (residual <= 1.5) step = magnitude;
    else if (residual <= 3.5) step = 2 * magnitude;
    else if (residual <= 7.5) step = 5 * magnitude;
    else step = 10 * magnitude;
    const first = Math.ceil(dataMin / step) * step;
    const last = Math.ceil(dataMax / step) * step;
    const ticks: number[] = [];
    for (let t = first; t <= last + step * 0.001; t += step) {
      ticks.push(Math.round(t * 10) / 10);
    }
    return ticks;
  }

  /** Current visible time window: by default the time span of all data points */
  const range = useMemo<TimeRange>(() => {
    if (data.length === 0) return { start: 0, end: 1 };
    let min = Number(data[0]?.[xKey]) || 0;
    let max = min;
    for (const d of data) {
      const t = Number(d[xKey]) || 0;
      if (t < min) min = t;
      if (t > max) max = t;
    }
    if (max <= min) max = min + 1;
    const v = view ?? { start: min, end: max };
    return {
      start: Math.min(v.start, max),
      end: Math.max(v.end, min),
    };
  }, [data, xKey, view]);

  /** Take the points whose time falls in the visible window (sampling when excessive) and return a { t, index } array */
  const visiblePts = useMemo(() => {
    const pts: { t: number; i: number }[] = [];
    for (let i = 0; i < data.length; i++) {
      const t = Number(data[i][xKey]) || 0;
      if (t < range.start || t > range.end) continue;
      pts.push({ t, i });
    }
    if (pts.length > MAX_DRAW_POINTS) {
      const stride = Math.ceil(pts.length / MAX_DRAW_POINTS);
      const sampled: { t: number; i: number }[] = [];
      for (let k = 0; k < pts.length; k += stride) sampled.push(pts[k]);
      const last = pts[pts.length - 1];
      if (sampled[sampled.length - 1].i !== last.i) sampled.push(last);
      return sampled;
    }
    return pts;
  }, [data, xKey, range]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth;
    const h = height;
    canvas.width = w * dpr;
    canvas.height = h * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const plotH = h - bottomPad;
    const span = Math.max(range.end - range.start, MIN_SPAN);
    const tx = (t: number) => ((t - range.start) / span) * w;

    // Background grid
    ctx.strokeStyle = "rgba(148,163,184,0.12)";
    ctx.lineWidth = 1;
    for (let i = 0; i <= 4; i++) {
      const y = (plotH / 4) * i;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(w, y);
      ctx.stroke();
    }

    if (visiblePts.length < 2) {
      ctx.fillStyle = "rgba(148,163,184,0.6)";
      ctx.font = "12px ui-sans-serif, system-ui";
      ctx.fillText(t("chart.waiting"), 12, plotH / 2);
    } else {
      // Each series is normalized independently (based on the values inside the visible window)
      for (const s of series) {
        let max = 1;
        for (const p of visiblePts) {
          const v = Number(data[p.i][s.key]) || 0;
          if (v > max) max = v;
        }
        const yv = (v: number) => plotH - (v / max) * (plotH - 12) - 6;

        ctx.strokeStyle = s.color;
        ctx.lineWidth = 2;
        ctx.lineJoin = "round";
        ctx.beginPath();
        visiblePts.forEach((p, idx) => {
          const x = tx(p.t);
          const y = yv(Number(data[p.i][s.key]) || 0);
          if (idx === 0) ctx.moveTo(x, y);
          else ctx.lineTo(x, y);
        });
        ctx.stroke();

        // End dot + series maximum label
        const last = visiblePts[visiblePts.length - 1];
        const lastX = tx(last.t);
        const lastY = yv(Number(data[last.i][s.key]) || 0);
        ctx.fillStyle = s.color;
        ctx.beginPath();
        ctx.arc(lastX, lastY, 3.5, 0, Math.PI * 2);
        ctx.fill();

        ctx.font = "10px ui-monospace, monospace";
        const label = `${s.format ? s.format(max) : max}`;
        ctx.fillText(
          label,
          Math.max(
            4,
            Math.min(w - ctx.measureText(label).width - 4, lastX + 7),
          ),
          Math.max(10, lastY - 6),
        );
      }
    }

    // ─── X-axis ticks ──────────────────────────────────
    const ticks = computeTicks(range.start, range.end);
    ctx.strokeStyle = "rgba(148,163,184,0.25)";
    ctx.fillStyle = "rgba(148,163,184,0.8)";
    ctx.font = "10px ui-monospace, monospace";
    ctx.textAlign = "center";
    for (const tick of ticks) {
      const x = tx(tick);
      if (x < 0 || x > w) continue;
      ctx.beginPath();
      ctx.moveTo(x, plotH + 1);
      ctx.lineTo(x, plotH + 5);
      ctx.stroke();
      const formatted = Number.isInteger(tick)
        ? `${tick}s`
        : `${tick.toFixed(1)}s`;
      ctx.fillText(formatted, x, plotH + 15);
    }
    ctx.textAlign = "start";

    // ─── Drag-selection box ──────────────────────────────────
    if (dragSel) {
      const x0 = Math.min(dragSel.x0, dragSel.x1);
      const x1 = Math.max(dragSel.x0, dragSel.x1);
      ctx.fillStyle = "rgba(56,189,248,0.15)";
      ctx.fillRect(x0, 0, x1 - x0, plotH);
      ctx.strokeStyle = "rgba(56,189,248,0.7)";
      ctx.lineWidth = 1;
      ctx.strokeRect(x0, 0, x1 - x0, plotH);
    }

    // ─── Hover overlay ─────────────────────────────
    if (hover && hover.index >= 0 && hover.index < data.length) {
      const hx = tx(Number(data[hover.index][xKey]) || 0);
      ctx.strokeStyle = "rgba(148,163,184,0.35)";
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 4]);
      ctx.beginPath();
      ctx.moveTo(hx, 0);
      ctx.lineTo(hx, plotH);
      ctx.stroke();
      ctx.setLineDash([]);

      for (const s of series) {
        let max = 1;
        for (const p of visiblePts) {
          const v = Number(data[p.i][s.key]) || 0;
          if (v > max) max = v;
        }
        const py =
          plotH -
          ((Number(data[hover.index][s.key]) || 0) / max) * (plotH - 12) -
          6;
        ctx.fillStyle = s.color;
        ctx.beginPath();
        ctx.arc(hx, py, 4, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = "#fff";
        ctx.beginPath();
        ctx.arc(hx, py, 2, 0, Math.PI * 2);
        ctx.fill();
      }
    }
  }, [
    data,
    series,
    height,
    xKey,
    hover,
    view,
    dragSel,
    range,
    visiblePts,
    t,
    locale,
  ]);

  // Wheel zoom (a native listener; passive: false is required for preventDefault)
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const rect = canvas.getBoundingClientRect();
      const w = rect.width;
      const mx = e.clientX - rect.left;
      const span = Math.max(range.end - range.start, MIN_SPAN);
      const t0 = range.start + (mx / w) * span;
      const factor = e.deltaY < 0 ? 0.7 : 1.4;
      const newSpan = Math.max(span * factor, MIN_SPAN);
      // Zoom anchored on the time under the cursor
      const start = t0 - (t0 - range.start) * (newSpan / span);
      setView({ start, end: start + newSpan });
    };
    canvas.addEventListener("wheel", onWheel, { passive: false });
    return () => canvas.removeEventListener("wheel", onWheel);
  }, [range, setView]);

  const timeAt = useCallback(
    (clientX: number) => {
      const canvas = canvasRef.current;
      if (!canvas) return 0;
      const rect = canvas.getBoundingClientRect();
      const span = Math.max(range.end - range.start, MIN_SPAN);
      return range.start + ((clientX - rect.left) / rect.width) * span;
    },
    [range],
  );

  const handleMouseDown = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      if (e.button !== 0) return;
      const rect = e.currentTarget.getBoundingClientRect();
      dragStartRef.current = { x: e.clientX - rect.left };
      setHover(null);
    },
    [],
  );

  const handleMouseMove = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      const canvas = canvasRef.current;
      if (!canvas) return;
      const rect = canvas.getBoundingClientRect();
      const mx = e.clientX - rect.left;

      if (dragStartRef.current) {
        // Drag-select a range
        setDragSel({ x0: dragStartRef.current.x, x1: mx });
        return;
      }

      const targetTime = timeAt(e.clientX);
      let best = -1;
      let bestDist = Infinity;
      for (const p of visiblePts) {
        const dist = Math.abs(p.t - targetTime);
        if (dist < bestDist) {
          bestDist = dist;
          best = p.i;
        }
      }
      if (best < 0) {
        setHover(null);
        return;
      }
      const values = series.map((s) => ({
        key: s.key,
        label: s.label,
        value: Number(data[best]?.[s.key]) || 0,
        color: s.color,
      }));
      setHover({ x: mx, index: best, values });
    },
    [data, series, timeAt, visiblePts],
  );

  const handleMouseUp = useCallback(() => {
    if (!dragStartRef.current) return;
    if (dragSel && Math.abs(dragSel.x1 - dragSel.x0) > 4) {
      const canvas = canvasRef.current;
      if (canvas) {
        const rect = canvas.getBoundingClientRect();
        const span = Math.max(range.end - range.start, MIN_SPAN);
        const t0 =
          range.start + (Math.min(dragSel.x0, dragSel.x1) / rect.width) * span;
        const t1 =
          range.start + (Math.max(dragSel.x0, dragSel.x1) / rect.width) * span;
        if (t1 - t0 >= MIN_SPAN) setView({ start: t0, end: t1 });
      }
    }
    dragStartRef.current = null;
    setDragSel(null);
  }, [dragSel, range]);

  const handleMouseLeave = useCallback(() => {
    dragStartRef.current = null;
    setDragSel(null);
    setHover(null);
  }, []);

  const handleDoubleClick = useCallback(() => {
    setView(null);
  }, []);

  const tooltipEl = hover && (
    <div
      className="pointer-events-none absolute z-20 rounded-md border border-border bg-popover px-2.5 py-1.5 text-xs shadow-lg"
      style={{
        left: Math.min(
          hover.x + 10,
          (containerRef.current?.clientWidth ?? 400) - 140,
        ),
        top: 8,
      }}
    >
      <div className="mb-0.5 text-xs text-muted-foreground">
        {typeof data[hover.index]?.[xKey] === "number"
          ? `${(data[hover.index][xKey] as number).toFixed(1)}s`
          : `#${hover.index + 1}`}
      </div>
      {hover.values.map((v) => (
        <div key={v.key} className="flex items-center gap-1.5">
          <span
            className="h-2 w-2 rounded-full shrink-0"
            style={{ background: v.color }}
          />
          <span className="text-muted-foreground">{v.label}:</span>
          <span className="font-mono font-medium text-foreground tabular-nums">
            {series.find((s) => s.key === v.key)?.format?.(v.value) ?? v.value}
          </span>
        </div>
      ))}
    </div>
  );

  return (
    <div ref={containerRef} className="relative">
      {view && (
        <button
          onClick={() => setView(null)}
          className="absolute right-1 top-1 z-20 rounded border border-border bg-card/90 px-2 py-0.5 text-xs text-muted-foreground hover:text-foreground"
        >
          {t("chart.showAll")}
        </button>
      )}
      <canvas
        ref={canvasRef}
        style={{ width: "100%", height, cursor: "crosshair" }}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseLeave}
        onDoubleClick={handleDoubleClick}
      />
      {tooltipEl}
      <div className="mt-1 flex flex-wrap items-center gap-3">
        {series.map((s) => (
          <div
            key={s.key}
            className="flex items-center gap-1 text-xs text-muted-foreground"
          >
            <span
              className="h-2 w-2 rounded-full"
              style={{ background: s.color }}
            />
            {s.label}
          </div>
        ))}
        {view && (
          <span className="ml-auto text-xs text-muted-foreground">
            {t("chart.hint")}
          </span>
        )}
      </div>
    </div>
  );
}
