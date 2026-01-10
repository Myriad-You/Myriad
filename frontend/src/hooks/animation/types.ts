/**
 * 动画协调器类型定义
 */

/** 动画优先级 */
export enum AnimationPriority {
  /** 页面级 - 立即执行，不进队列 */
  PAGE = 0,
  /** 区块级 - 高优先级队列 */
  SECTION = 1,
  /** 组件级 - 普通队列 */
  COMPONENT = 2,
  /** 元素级 - 可延迟/跳过 */
  ELEMENT = 3,
}

/** 动画状态 */
export enum AnimationState {
  /** 等待页面就绪 */
  WAITING = 'waiting',
  /** 已调度，等待执行 */
  SCHEDULED = 'scheduled',
  /** 可以开始动画 */
  READY = 'ready',
  /** 动画进行中 */
  RUNNING = 'running',
  /** 动画完成 */
  COMPLETED = 'completed',
  /** 已跳过 */
  SKIPPED = 'skipped',
}

/** 调度策略 */
export enum ScheduleStrategy {
  /** 立即执行，不进队列 */
  IMMEDIATE = 'immediate',
  /** 优先级队列 */
  PRIORITY = 'priority',
  /** 懒执行，视口内才调度 */
  LAZY = 'lazy',
  /** 批量执行 */
  BATCH = 'batch',
}

/** 动画配置 */
export interface AnimationConfig {
  /** 唯一标识 */
  id: string
  /** 优先级 */
  priority: AnimationPriority
  /** 分组ID（用于交错动画） */
  groupId?: string
  /** 在组内的索引 */
  index?: number
  /** 延迟时间(ms) */
  delay?: number
  /** 持续时间(ms) */
  duration?: number
  /** 是否可跳过 */
  canSkip?: boolean
}

/** 元素动画选项 */
export interface ElementAnimationOptions {
  /** 分组ID */
  groupId?: string
  /** 在组内的索引 */
  index?: number
  /** 交错延迟基数(ms) */
  staggerDelay?: number
  /** 是否等待页面就绪 */
  waitForPage?: boolean
}

/** 监听器类型 */
export type AnimationListener = (state: AnimationState) => void

/** 取消订阅函数 */
export type Unsubscribe = () => void

/** 协调器配置 */
export interface CoordinatorConfig {
  /** 基础并发数（稳态） */
  baseConcurrent: number
  /** 爆发并发数（页面切换/首次加载） */
  burstConcurrent: number
  /** 爆发持续时间(ms) */
  burstDuration: number
  /** 最小间隔(ms) */
  minInterval: number
  /** 默认交错延迟(ms) */
  defaultStaggerDelay: number
  /** 刷新间隔(ms)，限制RAF频率 */
  flushInterval: number
  /** 循环动画最大槽位数 */
  maxLoopSlots?: number
}

/** 默认配置 */
export const DEFAULT_CONFIG: CoordinatorConfig = {
  baseConcurrent: 16, // 稳态最多 16 个并发（从 12 提升）
  burstConcurrent: 48, // 爆发时最多 48 个并发（从 32 提升）
  burstDuration: 5000, // 爆发持续 5 秒
  minInterval: 16,
  defaultStaggerDelay: 35, // 略微减少交错延迟，加快首屏
  flushInterval: 16,
}
