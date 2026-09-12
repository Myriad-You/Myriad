export enum AnimationPriority {
  /** 立即执行，不占槽。 */
  PAGE = 0,
  SECTION = 1,
  COMPONENT = 2,
  ELEMENT = 3,
}

export enum AnimationState {
  WAITING = 'waiting',
  SCHEDULED = 'scheduled',
  READY = 'ready',
  RUNNING = 'running',
  COMPLETED = 'completed',
  SKIPPED = 'skipped',
}

export enum ScheduleStrategy {
  IMMEDIATE = 'immediate',
  PRIORITY = 'priority',
  /** 视口内才调度。 */
  LAZY = 'lazy',
  BATCH = 'batch',
}

export interface AnimationConfig {
  id: string
  priority: AnimationPriority
  groupId?: string
  index?: number
  delay?: number
  duration?: number
  canSkip?: boolean
}

export interface ElementAnimationOptions {
  groupId?: string
  index?: number
  staggerDelay?: number
  waitForPage?: boolean
}

export type AnimationListener = (state: AnimationState) => void

export type Unsubscribe = () => void

export interface CoordinatorConfig {
  baseConcurrent: number
  burstConcurrent: number
  burstDuration: number
  minInterval: number
  defaultStaggerDelay: number
  flushInterval: number
  maxLoopSlots?: number
}

export const DEFAULT_CONFIG: CoordinatorConfig = {
  baseConcurrent: 16,
  burstConcurrent: 48,
  burstDuration: 5000,
  minInterval: 16,
  defaultStaggerDelay: 35,
  flushInterval: 16,
}
