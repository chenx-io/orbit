import type {
  Collection,
  DataModel,
  Environment,
  HistoryEntry,
  HttpRequest,
  Scenario,
  Workspace,
} from "./types";
import {
  getUiLocale,
  setUiLocale,
  translateIn,
  type UiLocale,
} from "@/lib/localeDict";

let _n = 0;
export function uid(prefix = "id"): string {
  _n += 1;
  return `${prefix}-${Date.now().toString(36)}-${_n.toString(36)}`;
}

/**
 * i18n keys backing the built-in seed names.
 *
 * The seeds below carry structure only; every user-visible label is resolved through the active
 * UI locale at creation time. Nothing outside this module should read a name off the skeletons.
 */
export const SEED_KEYS = {
  workspaceName: "seed.workspace.name",
  workspaceDescription: "seed.workspace.description",
  envDev: "seed.env.dev",
  envStaging: "seed.env.staging",
  envProd: "seed.env.prod",
  collectionDefault: "seed.collection.default",
} as const;

interface WorkspaceSeed {
  id: string;
  nameKey: string;
  descriptionKey: string;
  color: string;
  createdAt: number;
  sortIndex: number;
}

interface EnvironmentSeed {
  id: string;
  nameKey: string;
  variables: Record<string, string>;
}

const WORKSPACE_SEEDS: WorkspaceSeed[] = [
  {
    id: "ws-default",
    nameKey: SEED_KEYS.workspaceName,
    descriptionKey: SEED_KEYS.workspaceDescription,
    color: "#71717a",
    createdAt: 0,
    sortIndex: 0,
  },
];

const ENVIRONMENT_SEEDS: EnvironmentSeed[] = [
  {
    id: "env-dev",
    nameKey: SEED_KEYS.envDev,
    variables: { base_url: "http://127.0.0.1:9090" },
  },
  {
    id: "env-staging",
    nameKey: SEED_KEYS.envStaging,
    variables: { base_url: "https://staging.example.com" },
  },
  {
    id: "env-prod",
    nameKey: SEED_KEYS.envProd,
    variables: { base_url: "https://api.example.com" },
  },
];

/** Workspace id every built-in seed entity belongs to. */
export const SEED_WORKSPACE_ID = "ws-default";

/** Environment id activated on a fresh install. */
export const SEED_ACTIVE_ENV_ID = "env-dev";

/**
 * Record the active UI locale so seed data created afterwards uses localized names.
 *
 * Delegates to `lib/localeDict` so the data layer has a single source of truth for the active
 * locale (also used by `t()` in `data/types.ts`). Only affects data created *after* the change,
 * so existing snapshots keep the names they were created with.
 */
export function setSeedLocale(locale: string | undefined): void {
  setUiLocale(locale);
}

/** Current locale used by the seed factories. */
export function seedLocale(): UiLocale {
  return getUiLocale();
}

/**
 * Built-in workspace seed (first-run fallback: aligned with the backend default workspace, only
 * one `ws-default` is created). Existing snapshots / migrations win via `applyPersisted`.
 */
export function seedWorkspaces(locale: string = getUiLocale()): Workspace[] {
  return WORKSPACE_SEEDS.map((w) => ({
    id: w.id,
    name: translateIn(locale, w.nameKey),
    description: translateIn(locale, w.descriptionKey),
    color: w.color,
    createdAt: w.createdAt,
    sortIndex: w.sortIndex,
  }));
}

/** Built-in environments (dev / staging / prod) attached to the default workspace. */
export function seedEnvironments(
  locale: string = getUiLocale(),
): Environment[] {
  return ENVIRONMENT_SEEDS.map((e) => ({
    id: e.id,
    name: translateIn(locale, e.nameKey),
    workspaceId: SEED_WORKSPACE_ID,
    variables: { ...e.variables },
    secrets: {},
  }));
}

/**
 * Default collection (the "default space"): guarantees API management always has at least one
 * usable space. Injected on initial launch, when the snapshot has no collection, and when the
 * user deletes the last one.
 */
export function defaultCollection(locale: string = getUiLocale()): Collection {
  return {
    id: "col-default",
    name: translateIn(locale, SEED_KEYS.collectionDefault),
    workspaceId: SEED_WORKSPACE_ID,
    kind: "http",
    items: [],
  };
}

/** Collection list seed (a single default collection). */
export function seedCollections(locale: string = getUiLocale()): Collection[] {
  return [defaultCollection(locale)];
}

export const seedRequests: HttpRequest[] = [];

export const seedModels: DataModel[] = [];

export const seedScenarios: Scenario[] = [];

export const seedHistory: HistoryEntry[] = [];
