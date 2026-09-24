// App-level UI state: locale, theme, active module, analytics, feedback, dialog switches.
import type { StateCreator } from "zustand";
import type {
  AnalyticsEvent,
  DataLocale,
  FeedbackItem,
  Locale,
  ModuleKey,
} from "@/data/types";
import { setSeedLocale, uid } from "@/data/seed";
import type { AppState } from "../types";

export interface AppSlice {
  activeModule: ModuleKey;
  locale: Locale;
  /** Dynamic-value data locale (the locale used to generate fake data: zh/en/ja); drives the dynamic value dialog and reference docs */
  dataLocale: DataLocale;
  theme: "light" | "dark" | "system";
  /** How requests are executed: locally, or through agents (null = spread across all available agents; array = a specific agent subset) */
  executionTarget: { mode: "local" | "agent"; agentIds: string[] | null };

  /** Currently selected grpc collection tree node (package / service), shown in the right-hand editor area */
  activeGrpcNode: {
    collectionId: string;
    type: "grpc-package" | "grpc-service";
    id: string;
  } | null;

  analytics: Record<AnalyticsEvent, number>;
  feedback: FeedbackItem[];

  ui: {
    sidebarCollapsed: boolean;
    importOpen: boolean;
    scenarioImportOpen: boolean;
    envEditorOpen: boolean;
    mockOpen: boolean;
    /** New grpc collection dialog */
    grpcDialogOpen: boolean;
  };

  setActiveModule: (m: ModuleKey) => void;
  setLocale: (l: Locale) => void;
  setDataLocale: (l: DataLocale) => void;
  setTheme: (t: "light" | "dark" | "system") => void;
  setExecutionTarget: (t: {
    mode: "local" | "agent";
    agentIds: string[] | null;
  }) => void;
  setActiveGrpcNode: (
    n: {
      collectionId: string;
      type: "grpc-package" | "grpc-service";
      id: string;
    } | null,
  ) => void;
  track: (e: AnalyticsEvent) => void;
  addFeedback: (f: Omit<FeedbackItem, "id" | "createdAt">) => void;
  toggleSidebar: () => void;
  setImportOpen: (o: boolean) => void;
  setScenarioImportOpen: (o: boolean) => void;
  setEnvEditorOpen: (o: boolean) => void;
  setMockOpen: (o: boolean) => void;
  setGrpcDialogOpen: (o: boolean) => void;
}

export const createAppSlice: StateCreator<AppState, [], [], AppSlice> = (
  set,
  get,
) => ({
  activeModule: "api",
  locale: "zh-CN",
  dataLocale: "zh",
  theme: "system",
  executionTarget: { mode: "local", agentIds: null },
  activeGrpcNode: null,

  analytics: {
    module_view: 0,
    request_send: 0,
    load_start: 0,
    load_stop: 0,
    scenario_run: 0,
    plugin_install: 0,
    import: 0,
    feedback: 0,
  },
  feedback: [],

  ui: {
    sidebarCollapsed: false,
    importOpen: false,
    scenarioImportOpen: false,
    envEditorOpen: false,
    mockOpen: false,
    grpcDialogOpen: false,
  },

  setActiveModule: (m) => {
    set({ activeModule: m });
    get().track("module_view");
  },
  setLocale: (l) => {
    // Keep the data-layer seed names in sync with the UI locale (affects newly created data only).
    setSeedLocale(l);
    set({ locale: l });
  },
  setDataLocale: (l) => set({ dataLocale: l }),
  setTheme: (t) => set({ theme: t }),
  setExecutionTarget: (executionTarget) => set({ executionTarget }),
  setActiveGrpcNode: (activeGrpcNode) => set({ activeGrpcNode }),

  track: (e) =>
    set((s) => ({ analytics: { ...s.analytics, [e]: s.analytics[e] + 1 } })),
  addFeedback: (f) =>
    set((s) => ({
      feedback: [{ ...f, id: uid("fb"), createdAt: Date.now() }, ...s.feedback],
      analytics: { ...s.analytics, feedback: s.analytics.feedback + 1 },
    })),

  toggleSidebar: () =>
    set((s) => ({ ui: { ...s.ui, sidebarCollapsed: !s.ui.sidebarCollapsed } })),
  setImportOpen: (o) => set((s) => ({ ui: { ...s.ui, importOpen: o } })),
  setScenarioImportOpen: (o) =>
    set((s) => ({ ui: { ...s.ui, scenarioImportOpen: o } })),
  setEnvEditorOpen: (o) => set((s) => ({ ui: { ...s.ui, envEditorOpen: o } })),
  setMockOpen: (o) => set((s) => ({ ui: { ...s.ui, mockOpen: o } })),
  setGrpcDialogOpen: (o) =>
    set((s) => ({ ui: { ...s.ui, grpcDialogOpen: o } })),
});
