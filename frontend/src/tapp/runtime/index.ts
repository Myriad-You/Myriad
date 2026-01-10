/**
 * Tapp Runtime 模块导出
 */

// 类型导出
export type {
  SafeInsets,
  TappNotificationOptions,
  SandboxMode as TappSandboxMode,
  WidgetRenderProps,
} from './sandbox'
export { createTappBridge, TappBridge } from './TappBridge'
// 沙箱组件
export { TappPageSandbox } from './TappPageSandbox'
export type { TappPageSandboxProps } from './TappPageSandbox'

// 兼容性导出（保持向后兼容，TappSandbox 作为 TappPageSandbox 的别名）
export { TappPageSandbox as TappSandbox } from './TappPageSandbox'
export { createPermissionController, TappPermissionController } from './TappPermission'
export { getTappRuntime, TappRuntime } from './TappRuntime'
export { getTappScheduler, TappScheduler } from './TappScheduler'

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
} from './TappScheduler'

export { TappWidgetSandbox } from './TappWidgetSandbox'

export type { TappWidgetSandboxProps } from './TappWidgetSandbox'
