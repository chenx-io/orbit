import {
  BookOpen,
  Database,
  FlaskConical,
  Workflow,
  Gauge,
  LineChart,
  Puzzle,
  History,
  Network,
  type LucideIcon,
} from "lucide-react";
import type { ModuleKey } from "@/data/types";

export interface ModuleMeta {
  key: ModuleKey;
  i18nLabel: string;
  i18nShort: string;
  i18nDesc: string;
  icon: LucideIcon;
  accent: string;
}

export const MODULES: ModuleMeta[] = [
  {
    key: "api",
    i18nLabel: "nav.api",
    i18nShort: "nav.api",
    i18nDesc: "nav.desc.api",
    icon: FlaskConical,
    accent: "text-primary",
  },
  {
    key: "automation",
    i18nLabel: "nav.automation",
    i18nShort: "nav.automation",
    i18nDesc: "nav.desc.automation",
    icon: Workflow,
    accent: "text-violet-400",
  },
  {
    key: "load",
    i18nLabel: "nav.load",
    i18nShort: "nav.load",
    i18nDesc: "nav.desc.load",
    icon: Gauge,
    accent: "text-emerald-400",
  },
  {
    key: "performance",
    i18nLabel: "nav.performance",
    i18nShort: "nav.performance",
    i18nDesc: "nav.desc.performance",
    icon: LineChart,
    accent: "text-amber-400",
  },
  {
    key: "plugins",
    i18nLabel: "nav.plugins",
    i18nShort: "nav.plugins",
    i18nDesc: "nav.desc.plugins",
    icon: Puzzle,
    accent: "text-fuchsia-400",
  },
  {
    key: "history",
    i18nLabel: "nav.history",
    i18nShort: "nav.history",
    i18nDesc: "nav.desc.history",
    icon: History,
    accent: "text-cyan-400",
  },
  {
    key: "distributed",
    i18nLabel: "nav.distributed",
    i18nShort: "nav.distributed",
    i18nDesc: "nav.desc.distributed",
    icon: Network,
    accent: "text-indigo-400",
  },
  {
    key: "datasource",
    i18nLabel: "nav.datasource",
    i18nShort: "nav.datasource",
    i18nDesc: "nav.desc.datasource",
    icon: Database,
    accent: "text-sky-400",
  },
  {
    key: "actionlib",
    i18nLabel: "nav.actionlib",
    i18nShort: "nav.actionlib",
    i18nDesc: "nav.desc.actionlib",
    icon: BookOpen,
    accent: "text-violet-400",
  },
];

/** Workspace-scoped modules: data is isolated per workspace */
export const WORKSPACE_MODULES: ModuleMeta[] = MODULES.filter((m) =>
  ["api", "automation", "load", "performance", "history"].includes(m.key),
);

/** App-level modules: shared across workspaces (plugins / distributed), pinned to the "Global" group at the bottom of the sidebar */
export const GLOBAL_MODULES: ModuleMeta[] = MODULES.filter((m) =>
  ["plugins", "distributed"].includes(m.key),
);
