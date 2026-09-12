import { coordinator } from './coordinator'

export {
  useAnimationFrame,
  useBatchedDom,
  useDebounce,
  useElementSize,
  useIdleEffect,
  useInView,
  useLazyLoad,
  usePageVisible as usePageVisibleAtomic,
  useThrottle,
  useVisibilityInterval as useVisibilityIntervalAtomic,
} from './atomicHooks'

export { coordinator } from './coordinator'

export { default as AnimationCoordinator } from './coordinator'

export {
  batchRead as batchReadAtomic,
  batchWrite as batchWriteAtomic,
  cancelIdle,
  destroy as destroyAtomicCore,
  getStats as getAtomicStats,
  getCurrentPageId,
  isPageVisible,
  isSchedulerActive,
  now,
  observeIntersection as observeIntersectionAtomic,
  observeResize as observeResizeAtomic,
  onVisibility,
  pause as pauseScheduler,
  refreshNow,
  registerPageCleanup,
  resume as resumeScheduler,
  runPageCleanup,
  scheduleIdle,
  scheduleTask,
  startPage,
  yieldToMain as yieldToMainAtomic,
} from './core'

export {
  Feature,
  getFeatureList,
  hasFeature,
  PAGE_FEATURES,
} from './pageFeatures'

export {
  awaitLaneSwap,
  BREW_TAG_ENTER_MS,
  BREW_TAG_EXIT_MS,
  BREW_TAG_STAGGER_MS,
  brewAnimationPresets,
  brewMotionClaim,
  brewMotionLane,
  brewMotionQuiet,
  brewMotionReset,
  brewSurfaceSwapWait,
  brewTagDelay,
  brewTagQuiet,
  brewTagSwapWait,
  chipEnterFrames,
  chipExitFrames,
  cleanupBrew,
  diffChipKeys,
  getBrewTransition,
  planChipLaneSwap,
  playBrewChipEnter,
  playBrewChipExit,
  playBrewSurfaceExit,
  playBrewVeilEnter,
  playBrewVeilExit,
  shouldPlayChipEnter,
  useBrewAnimationConfig,
  useBrewScheduler,
  useBrewTag,
} from './pages/brew'

export {
  cleanupHome,
  useHomeIdle,
  useHomeRaf,
  useHomeResize,
  useHomeResizeObserver,
  useHomeScheduler,
  useHomeVisibility,
  useHomeVisibilityInterval,
} from './pages/home'

export {
  cleanupLibrary,
  useLibraryInfiniteScroll,
  useLibraryIntersectionObserver,
  useLibraryInView,
  useLibraryLazyLoad,
  useLibraryPrefetch,
  useLibraryResize,
  useLibraryScheduler,
} from './pages/library'

export {
  cleanupReports,
  useReportsBatchDom,
  useReportsInterval,
  useReportsRaf,
  useReportsRafThrottle,
  useReportsScheduler,
  useReportsTimeout,
  useReportsVisibility,
  useReportsVisibilityInterval,
} from './pages/reports'

export {
  useConfigScheduler,
  useDetailsScheduler,
  useLoginScheduler,
  useSetupScheduler,
  useSimpleDebounce,
  useSimplePageScheduler,
  useSimpleThrottle,
  useSimpleTimeout,
} from './pages/simple'

export {
  cleanupTapp,
  useTappScheduler,
  useTappStagger,
  useTappVisibility,
} from './pages/tapp'

export type {
  AnimationConfig,
  AnimationListener,
  CoordinatorConfig,
  ElementAnimationOptions,
  Unsubscribe,
} from './types'
export {
  AnimationPriority,
  AnimationState,
  DEFAULT_CONFIG,
  ScheduleStrategy,
} from './types'

export {
  type AnimationLifecycleOptions,
  AnimationLifecyclePhase,
  type AnimationLifecycleResult,
  type BatchAnimationOptions,
  type BatchAnimationResult,
  useAnimationLifecycle,
  useBatchAnimationLifecycle,
} from './useAnimationLifecycle'
export { useElementAnimation } from './useElementAnimation'
export { useLoopAnimation } from './useLoopAnimation'
export { usePageReady } from './usePageReady'

export { pageTransitionManager, usePageTransition } from './usePageTransition'

export { usePageScheduler, useRouteScheduler } from './useRouteScheduler'

export { useStaggerAnimation } from './useStaggerAnimation'

export {
  usePageVisible,
  useVisibilityInterval,
  useVisibilityTimeout,
} from './useVisibilityPause'

export function configureAnimationCoordinator(
  config: Partial<import('./types').CoordinatorConfig>,
) {
  coordinator.updateConfig(config)
}

export function startFpsMonitor() {
  coordinator.startFpsMonitor()
}

export function stopFpsMonitor() {
  coordinator.stopFpsMonitor()
}

export function getFps(): number {
  return coordinator.getFps()
}

export function isLowFps(): boolean {
  return coordinator.isLowFps()
}

export function getFrameStats() {
  return coordinator.getFrameStats()
}

export function resetFrameStats() {
  coordinator.resetFrameStats()
}

export function getDetectedRefreshRate(): number {
  return coordinator.getDetectedRefreshRate()
}

export function batchRead(callback: () => void): void {
  coordinator.batchRead(callback)
}

export function batchWrite(callback: () => void): void {
  coordinator.batchWrite(callback)
}

export function yieldToMain(): Promise<void> {
  return coordinator.yieldToMain()
}

export function shouldYield(): boolean {
  return coordinator.shouldYield()
}

export function observeResize(
  element: Element,
  callback: (entry: ResizeObserverEntry) => void,
  options?: { immediate?: boolean },
): () => void {
  return coordinator.observeResize(element, callback, options)
}

export function unobserveResize(element: Element): void {
  coordinator.unobserveResize(element)
}

export function getObservedElementCount(): number {
  return coordinator.getObservedElementCount()
}

export function getCachedSize(
  element: Element,
): { width: number; height: number } | null {
  return coordinator.getCachedSize(element)
}

export function observeIntersection(
  element: Element,
  callback: (entry: IntersectionObserverEntry) => void,
  options?: { threshold?: number; rootMargin?: string },
): () => void {
  return coordinator.observeIntersection(element, callback, options)
}

export function unobserveIntersection(element: Element): void {
  coordinator.unobserveIntersection(element)
}

export function getIntersectionObservedCount(): number {
  return coordinator.getIntersectionObservedCount()
}

export function getIntersectionObserverCount(): number {
  return coordinator.getIntersectionObserverCount()
}

export function onVisibilityChange(
  callback: (isVisible: boolean) => void,
): () => void {
  return coordinator.onVisibilityChange(callback)
}

export function getPageVisibility(): boolean {
  return coordinator.getPageVisibility()
}

export function scheduleIdleTask(
  id: string,
  task: () => void,
  options?: {
    timeout?: number
    priority?: 'low' | 'normal' | 'high'
    dedupe?: boolean
  },
): () => void {
  return coordinator.scheduleIdleTask(id, task, options)
}

export function cancelIdleTask(id: string): boolean {
  return coordinator.cancelIdleTask(id)
}

export function getIdleTaskCount(): number {
  return coordinator.getIdleTaskCount()
}
