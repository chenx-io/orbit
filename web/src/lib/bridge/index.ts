// Unified entry point of the Tauri bridge layer.
// Layout (after the split):
// - client.ts       → low-level call primitives (isTauri / tauriInvoke / apiPost / apiGet / apiDelete)
// - request.ts      → dynamic value generation + request execution (proxy)
// - mock.ts         → mock service (interfaces + expectation models)
// - snapshot.ts     → local snapshot persistence + file download
// - importExport.ts → import (cURL/Postman/OpenAPI) and export (request code / model validation)
// - loadtest.ts     → load-test runs + live metrics stream + scenario runs
// - reports.ts      → performance reports & baseline management
// - scenarioReports.ts → automation scenario run reports (persistence / history list)
// - ai.ts            → AI assistant (BYOK credentials / sessions / turns / event polling, desktop only)
export { isTauri } from "./client";
export {
  generateDynamicValue,
  resolveDynamicValues,
  executeRequest,
} from "./request";
export type { ProxyResponse } from "./request";
export {
  startMockServer,
  stopMockServer,
  getMockRules,
  saveMockInterface,
  deleteMockInterface,
  restoreMockRules,
} from "./mock";
export {
  SNAPSHOT_LS_KEY,
  saveSnapshotJson,
  loadSnapshotJson,
  clearSnapshotJson,
  exportSnapshotToFile,
  writeExportFile,
  downloadTextFile,
  dataLoadSnapshot,
  dataSaveSnapshot,
  dataClear,
  dataAddHistory,
  dataClearHistory,
} from "./snapshot";
export {
  parseImport,
  importScenario,
  readTextFile,
  exportRequest,
  exportCollection,
  validateResponseAgainstModel,
} from "./importExport";
export type {
  ImportedEndpoint,
  ImportedResponse,
  ImportedSchema,
  ImportParseResult,
  ExportFormat,
  ValidationResult,
} from "./importExport";
export {
  runLoadTest,
  stopLoadTest,
  connectLoadTestStream,
  drainScenarioProgress,
  exportLoadReport,
  runScenario,
} from "./loadtest";
export {
  listDataSources,
  upsertDataSource,
  removeDataSource,
  testDataSource,
  previewQuery,
  isSqlResult,
} from "./datasource";
export type {
  DataSourceTestReport,
  SqlPreview,
  RedisPreview,
} from "./datasource";
export type {
  LoadTestConfig,
  LoadTestResult,
  LoadStreamHandle,
  LoadReportExport,
} from "./loadtest";
export {
  sessionOpen,
  sessionSend,
  sessionClose,
  sessionMessages,
  connectSessionEvents,
  grpcReflect,
} from "./session";
export type {
  OpenSessionOptions,
  SessionOpenResult,
  SessionEvent,
  SessionMessage,
  SessionStreamHandle,
  GrpcReflectService,
  GrpcReflectMethod,
} from "./session";
export {
  saveReport,
  listReports,
  loadReport,
  deleteReport,
  exportSavedReport,
  setBaseline,
  unsetBaseline,
  listBaselines,
  renameReport,
} from "./reports";
export {
  saveScenarioReport,
  listScenarioReports,
  loadScenarioReport,
  deleteScenarioReport,
  clearScenarioReports,
  toReportSummary,
} from "./scenarioReports";
export {
  pluginList,
  pluginScan,
  pluginLoad,
  pluginUnload,
  pluginProtocols,
  pluginCodecs,
} from "./plugin";
export type {
  PluginDescriptor,
  PluginScanReport,
  PluginLoadResult,
} from "./plugin";
export {
  aiDesktopOnlyError,
  aiAbortTurn,
  aiApproveTool,
  aiConfigGet,
  aiConfigSave,
  aiCredentialList,
  aiCredentialRemove,
  aiCredentialSave,
  aiDrainEvents,
  aiListModels,
  aiReadDefinitionFile,
  aiSessionDelete,
  aiSessionList,
  aiSessionLoad,
  aiSessionSave,
  aiStartTurn,
  aiTestConnection,
  connectAiEvents,
} from "./ai";
export type {
  AiContextSummary,
  AiCredentialInput,
  AiCredentialView,
  AiDiffKind,
  AiEvent,
  AiFieldDiff,
  AiMessage,
  AiMode,
  AiPlanArtifact,
  AiPlanStep,
  AiPrefs,
  AiProposal,
  AiProposalAction,
  AiProviderKind,
  AiReference,
  AiReferenceFile,
  AiReferenceKind,
  AiSession,
  AiSessionSummary,
  AiStartTurnRequest,
  AiStartTurnResponse,
  AiTestConnectionResult,
  AiToolCard,
  AiToolKind,
  AiToolStatus,
  AiUsage,
} from "@/data/aiTypes";
export {
  AI_DEFAULT_BASE_URL,
  AI_DEFAULT_MODEL,
  AI_MODEL_PRESETS,
  DEFAULT_AI_PREFS,
} from "@/data/aiTypes";
