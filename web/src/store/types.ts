// Composed AppState type: merged from the domain slice interfaces.
// Uses the slice pattern recommended by zustand; each slice owns one domain.
import type { AppSlice } from "./slices/appSlice";
import type { CollectionSlice } from "./slices/collectionSlice";
import type { RequestSlice } from "./slices/requestSlice";
import type { RequestRunnerSlice } from "./slices/requestRunner";
import type { ModelSlice } from "./slices/modelSlice";
import type { EnvironmentSlice } from "./slices/environmentSlice";
import type { ActionLibrarySlice } from "./slices/actionLibrarySlice";
import type { DataSourceSlice } from "./slices/dataSourceSlice";
import type { HistorySlice } from "./slices/historySlice";
import type { ScenarioSlice } from "./slices/scenarioSlice";
import type { PluginSlice } from "./slices/pluginSlice";
import type { LoadTestSlice } from "./slices/loadTestSlice";
import type { WorkspaceSlice } from "./slices/workspaceSlice";
import type { AiSlice } from "./slices/aiSlice";

export type AppState = AppSlice &
  CollectionSlice &
  RequestSlice &
  RequestRunnerSlice &
  ModelSlice &
  EnvironmentSlice &
  ActionLibrarySlice &
  DataSourceSlice &
  HistorySlice &
  ScenarioSlice &
  PluginSlice &
  LoadTestSlice &
  WorkspaceSlice &
  AiSlice;
