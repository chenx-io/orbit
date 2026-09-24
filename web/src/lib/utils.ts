import { clsx, type ClassValue } from "clsx";
import { tFormat } from "@/lib/localeDict";
import { twMerge } from "tailwind-merge";
import type { ProtocolKind } from "@/data/types";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

// HTTP method colors (for badges / lists). Maintained centrally here; components must not define their own.
// Covers standard methods plus common extensions (WebDAV / cloud vendors etc.), falling back to gray when unmatched.
const METHOD_COLOR: Record<string, string> = {
  GET: "text-emerald-400",
  POST: "text-blue-400",
  PUT: "text-amber-400",
  DELETE: "text-rose-400",
  PATCH: "text-violet-400",
  HEAD: "text-cyan-400",
  OPTIONS: "text-slate-400",
  CONNECT: "text-orange-400",
  TRACE: "text-teal-400",
  COPY: "text-blue-400",
  MOVE: "text-indigo-400",
  LOCK: "text-lime-400",
  UNLOCK: "text-lime-400",
  MKCOL: "text-fuchsia-400",
  PROPFIND: "text-zinc-400",
  PROPPATCH: "text-zinc-400",
  SEARCH: "text-pink-400",
  REPORT: "text-purple-400",
  PURGE: "text-rose-400",
  LINK: "text-teal-400",
  UNLINK: "text-teal-400",
  BREW: "text-amber-400",
};

const METHOD_BG: Record<string, string> = {
  GET: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  POST: "bg-blue-500/15 text-blue-300 border-blue-500/30",
  PUT: "bg-amber-500/15 text-amber-300 border-amber-500/30",
  DELETE: "bg-rose-500/15 text-rose-300 border-rose-500/30",
  PATCH: "bg-violet-500/15 text-violet-300 border-violet-500/30",
  HEAD: "bg-cyan-500/15 text-cyan-300 border-cyan-500/30",
  OPTIONS: "bg-slate-500/15 text-slate-300 border-slate-500/30",
  CONNECT: "bg-orange-500/15 text-orange-300 border-orange-500/30",
  TRACE: "bg-teal-500/15 text-teal-300 border-teal-500/30",
  COPY: "bg-blue-500/15 text-blue-300 border-blue-500/30",
  MOVE: "bg-indigo-500/15 text-indigo-300 border-indigo-500/30",
  LOCK: "bg-lime-500/15 text-lime-300 border-lime-500/30",
  UNLOCK: "bg-lime-500/15 text-lime-300 border-lime-500/30",
  MKCOL: "bg-fuchsia-500/15 text-fuchsia-300 border-fuchsia-500/30",
  PROPFIND: "bg-zinc-500/15 text-zinc-300 border-zinc-500/30",
  PROPPATCH: "bg-zinc-500/15 text-zinc-300 border-zinc-500/30",
  SEARCH: "bg-pink-500/15 text-pink-300 border-pink-500/30",
  REPORT: "bg-purple-500/15 text-purple-300 border-purple-500/30",
  PURGE: "bg-rose-500/15 text-rose-300 border-rose-500/30",
  LINK: "bg-teal-500/15 text-teal-300 border-teal-500/30",
  UNLINK: "bg-teal-500/15 text-teal-300 border-teal-500/30",
  BREW: "bg-amber-500/15 text-amber-300 border-amber-500/30",
};

const DEFAULT_BG = "bg-slate-500/15 text-slate-300 border-slate-500/30";

export function methodColor(m?: string | null): string {
  return METHOD_COLOR[(m ?? "GET").toUpperCase()] ?? "text-muted-foreground";
}

export function methodBg(m?: string | null): string {
  return METHOD_BG[(m ?? "GET").toUpperCase()] ?? DEFAULT_BG;
}

/** Badge label for non-HTTP protocols */
export const PROTOCOL_LABEL: Record<string, string> = {
  http: "HTTP",
  websocket: "WS",
  grpc: "gRPC",
  tcp: "TCP",
  udp: "UDP",
  sse: "SSE",
  graphql: "GQL",
};

/** Protocol list selectable for "new request".
 * Only HTTP is kept — gRPC requests can only be created via "new collection → import proto/reflection";
 * websocket/tcp/udp/sse/graphql are not creatable yet (existing requests can still be opened and edited). */
export const PROTOCOL_OPTIONS: ProtocolKind[] = ["http"];

/** Badge color for non-HTTP protocols (HTTP keeps using methodColor) */
export function protocolColor(p: string): string {
  switch (p) {
    case "http":
      return "text-primary";
    case "websocket":
      return "text-emerald-500";
    case "grpc":
      return "text-purple-400";
    case "tcp":
      return "text-blue-400";
    case "udp":
      return "text-amber-400";
    case "sse":
      return "text-rose-400";
    case "graphql":
      return "text-fuchsia-400";
    default:
      return "text-muted-foreground";
  }
}

export function statusColor(status: number | null): string {
  if (status == null) return "text-muted-foreground";
  if (status < 300) return "text-emerald-400";
  if (status < 400) return "text-blue-400";
  if (status < 500) return "text-amber-400";
  return "text-rose-400";
}

export function formatBytes(bytes: number | null): string {
  if (bytes == null) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

export function formatTime(ts: number): string {
  const d = new Date(ts);
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** base64 string → human-readable size (B/KB/MB/GB). */
export function fileSize(base64: string): string {
  const bytes = Math.max(0, Math.floor((base64.length * 3) / 4));
  const units = ["B", "KB", "MB", "GB"];
  let v = bytes;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v.toFixed(v < 10 && u > 0 ? 1 : 0)} ${units[u]}`;
}

export function timeAgo(ts: number): string {
  const diff = Date.now() - ts;
  const s = Math.floor(diff / 1000);
  if (s < 60) return tFormat("time.secondsAgo", s);
  const m = Math.floor(s / 60);
  if (m < 60) return tFormat("time.minutesAgo", m);
  const h = Math.floor(m / 60);
  if (h < 24) return tFormat("time.hoursAgo", h);
  return formatTime(ts);
}
