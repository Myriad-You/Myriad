/**
 * Tapp 模块主入口
 */

// 示例 Tapp 导出
export { EXAMPLE_TAPPS, helloWorldTapp } from './examples'

// 页面导出
export {
  TappDetailPage,
  TappListPage,
  TappRunPage,
} from './pages'
// 运行时导出
export {
  createPermissionController,
  createTappBridge,
  getTappRuntime,
  getTappScheduler,
  TappBridge,
  TappPermissionController,
  TappRuntime,
  TappSandbox,
  TappScheduler,
} from './runtime'

export type {
  BackendAction,
  ExecutionTarget,
  MissedPolicy,
  RegisteredTask,
  RetryConfig,
  ScheduleConfig,
  ScheduleType,
  TaskCallback,
  TaskExecutionEvent,
  TaskExecutionStatus,
  TaskRegistrationOptions,
} from './runtime'

// 服务导出
export { OFFICIAL_STORE, RemoteStoreService } from './services/RemoteStoreService'

export type {
  RemoteApp,
  RemoteCategory,
  RemoteStoreIndex,
  RemoteStoreSource,
} from './services/RemoteStoreService'
// 类型导出
export * from './types'
