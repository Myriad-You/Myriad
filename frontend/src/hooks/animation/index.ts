/**
 * 动画协调系统
 *
 * 统一管理所有页面和元素动画的调度
 *
 * ## 架构层次
 *
 * ### 1. 页面级调度器 (pages/*.ts) - 🆕 推荐
 * - 每个页面独立的调度器 Hooks
 * - 只加载当前页面需要的功能
 * - Tree-shaking 友好，最小化打包体积
 *
 * ### 2. 原子化核心 (core.ts) - 轻量级，惰性初始化
 * - 零开销初始化：未使用的模块不会初始化
 * - 独立模块：可见性、任务调度、Observer、DOM批量
 * - 适合：性能敏感场景、按需加载
 *
 * ### 3. 原子化 Hooks (atomicHooks.ts) - 细粒度 React Hooks
 * - 基于 core.ts 封装
 * - Tree-shaking 友好
 * - 适合：轻量组件、单一功能
 *
 * ### 4. 完整协调器 (coordinator.ts) - 全功能（遗留）
 * - 页面级动画编排
 * - 交错动画、循环动画
 * - 帧率监控、性能统计
 * - 适合：复杂动画场景
 *
 * @example
 * ```tsx
 * // === 🆕 页面级调度器（推荐）===
 * // 首页
 * import { useHomeScheduler, useHomeResize } from '@hooks/animation/pages/home';
 * function Home() {
 *   useHomeScheduler();
 *   const { width } = useHomeResize(containerRef);
 *   // ...
 * }
 *
 * // 资料库
 * import { useLibraryScheduler, useLibraryLazyLoad } from '@hooks/animation/pages/library';
 * function LibraryItem() {
 *   const { ref, shouldLoad } = useLibraryLazyLoad();
 *   return <img ref={ref} src={shouldLoad ? url : placeholder} />;
 * }
 *
 * // 报告页
 * import { useReportsScheduler, useReportsVisibilityInterval } from '@hooks/animation/pages/reports';
 * function Reports() {
 *   useReportsScheduler();
 *   useReportsVisibilityInterval(() => nextCard(), 10000);
 *   // ...
 * }
 * ```
 */

// 页面级调度器
// 每个页面独立的调度器 Hooks，统一通过 barrel 导出

// Home
// 页面功能配置
// 便捷函数
import { coordinator } from './coordinator'

// 原子化 Hooks（按需使用）
// 这些 Hooks 基于 core.ts，细粒度且 tree-shakeable
export {
  // 动画帧
  useAnimationFrame,
  // DOM 批量操作
  useBatchedDom,
  useDebounce,
  // 尺寸观察
  useElementSize,
  // 空闲任务
  useIdleEffect,
  // 视口可见性 / 懒加载
  useInView,
  useLazyLoad,
  // 可见性 Hooks（使用原子化核心）
  usePageVisible as usePageVisibleAtomic,
  // 防抖节流
  useThrottle,
  useVisibilityInterval as useVisibilityIntervalAtomic,
} from './atomicHooks'

// 导出协调器
export { coordinator } from './coordinator'

export { default as AnimationCoordinator } from './coordinator'

// 原子化核心（轻量级，惰性初始化）
// 这些 API 来自 core.ts，具有零开销初始化特性
export {
  // DOM 批量操作
  batchRead as batchReadAtomic,
  batchWrite as batchWriteAtomic,
  cancelIdle,
  // 完全销毁
  destroy as destroyAtomicCore,
  // 调试统计
  getStats as getAtomicStats,
  getCurrentPageId,
  isPageVisible,
  isSchedulerActive,
  // 时间戳缓存
  now,
  observeIntersection as observeIntersectionAtomic,
  // Observer 管理（惰性初始化）
  observeResize as observeResizeAtomic,
  // 页面可见性（惰性初始化）
  onVisibility,
  pause as pauseScheduler,
  refreshNow,
  // 页面清理注册表（自注册模式）
  registerPageCleanup,
  resume as resumeScheduler,
  runPageCleanup,
  // 空闲任务调度器（惰性初始化）
  scheduleIdle,
  // MessageChannel 任务调度（惰性初始化）
  scheduleTask,
  // 页面生命周期管理
  startPage,
  yieldToMain as yieldToMainAtomic,
} from './core'

export {
  Feature,
  getFeatureList,
  hasFeature,
  PAGE_FEATURES,
} from './pageFeatures'

// Brew
export {
  brewAnimationPresets,
  cleanupBrew,
  getBrewTransition,
  useBrewAnimationConfig,
  useBrewCardStagger,
  useBrewScheduler,
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

// Library
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

// Reports
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

// Simple pages (Config, Login, Setup, Details)
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

// Tapp
export {
  cleanupTapp,
  useTappScheduler,
  useTappStagger,
  useTappVisibility,
} from './pages/tapp'

// 类型定义
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

// 新增：生命周期管理 Hook
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
// 导出 Hooks
export { pageTransitionManager, usePageTransition } from './usePageTransition'

// 新增：路由调度器整合 Hook
export { usePageScheduler, useRouteScheduler } from './useRouteScheduler'

export { useStaggerAnimation } from './useStaggerAnimation'

// 新增：可见性感知定时器 Hook
export {
  usePageVisible,
  useVisibilityInterval,
  useVisibilityTimeout,
} from './useVisibilityPause'

/**
 * 配置协调器
 */
export function configureAnimationCoordinator(
  config: Partial<import('./types').CoordinatorConfig>,
) {
  coordinator.updateConfig(config)
}

// 帧率管理 API

/**
 * 启动 FPS 监控
 */
export function startFpsMonitor() {
  coordinator.startFpsMonitor()
}

/**
 * 停止 FPS 监控
 */
export function stopFpsMonitor() {
  coordinator.stopFpsMonitor()
}

/**
 * 获取当前 FPS
 */
export function getFps(): number {
  return coordinator.getFps()
}

/**
 * 是否处于低帧率模式
 */
export function isLowFps(): boolean {
  return coordinator.isLowFps()
}

/**
 * 获取帧率统计
 * 用于性能监控面板，统一从调度器获取数据
 */
export function getFrameStats() {
  return coordinator.getFrameStats()
}

/**
 * 重置帧率统计
 */
export function resetFrameStats() {
  coordinator.resetFrameStats()
}

/**
 * 获取检测到的显示器刷新率
 * 通过前几帧的最小帧时间自动推断
 */
export function getDetectedRefreshRate(): number {
  return coordinator.getDetectedRefreshRate()
}

// DOM 批量操作 API

/**
 * 批量 DOM 读取
 * 将读取操作收集到队列，在下一帧统一执行，避免强制重排
 *
 * @example
 * ```ts
 * batchRead(() => {
 *   width = el.offsetWidth;
 *   height = el.offsetHeight;
 * });
 * ```
 */
export function batchRead(callback: () => void): void {
  coordinator.batchRead(callback)
}

/**
 * 批量 DOM 写入
 * 将写入操作收集到队列，在读取之后统一执行
 *
 * @example
 * ```ts
 * batchWrite(() => {
 *   el.style.width = width + 'px';
 *   el.style.height = height + 'px';
 * });
 * ```
 */
export function batchWrite(callback: () => void): void {
  coordinator.batchWrite(callback)
}

/**
 * 让出主线程
 * 用于长任务中断，避免阻塞
 */
export function yieldToMain(): Promise<void> {
  return coordinator.yieldToMain()
}

/**
 * 检查是否应该让出主线程
 */
export function shouldYield(): boolean {
  return coordinator.shouldYield()
}

// ResizeObserver 管理 API

/**
 * 观察元素尺寸变化
 * 使用共享的 ResizeObserver，比每个元素单独创建更高效
 *
 * 特性：
 * - 自动节流（50ms）
 * - 尺寸变化阈值过滤（<4px 忽略）
 * - 页面不可见时暂停
 * - 批量回调执行
 *
 * @param element 要观察的元素
 * @param callback 尺寸变化回调
 * @param options.immediate 是否立即测量一次
 * @returns 取消观察函数
 *
 * @example
 * ```ts
 * const unobserve = observeResize(element, (entry) => {
 *   const { width, height } = entry.contentRect;
 *   console.log('Size changed:', width, height);
 * }, { immediate: true });
 *
 * // 清理
 * unobserve();
 * ```
 */
export function observeResize(
  element: Element,
  callback: (entry: ResizeObserverEntry) => void,
  options?: { immediate?: boolean },
): () => void {
  return coordinator.observeResize(element, callback, options)
}

/**
 * 取消观察元素尺寸
 */
export function unobserveResize(element: Element): void {
  coordinator.unobserveResize(element)
}

/**
 * 获取观察中的元素数量
 */
export function getObservedElementCount(): number {
  return coordinator.getObservedElementCount()
}

/**
 * 获取元素的缓存尺寸（无需触发重排）
 */
export function getCachedSize(
  element: Element,
): { width: number; height: number } | null {
  return coordinator.getCachedSize(element)
}

// IntersectionObserver 管理 API

/**
 * 观察元素可见性变化
 * 使用共享的 IntersectionObserver 池，相同配置共享同一个 Observer
 *
 * 特性：
 * - 按配置分组共享 Observer
 * - 页面不可见时暂停
 * - 批量回调执行（使用 microtask）
 *
 * @param element 要观察的元素
 * @param callback 可见性变化回调
 * @param options.threshold 可见性阈值（0-1，默认 0）
 * @param options.rootMargin 根边距（默认 '0px'）
 * @returns 取消观察函数
 *
 * @example
 * ```ts
 * const unobserve = observeIntersection(element, (entry) => {
 *   if (entry.isIntersecting) {
 *     console.log('Element is visible');
 *   } else {
 *     console.log('Element is hidden');
 *   }
 * }, { threshold: 0.5, rootMargin: '100px' });
 *
 * // 清理
 * unobserve();
 * ```
 */
export function observeIntersection(
  element: Element,
  callback: (entry: IntersectionObserverEntry) => void,
  options?: { threshold?: number; rootMargin?: string },
): () => void {
  return coordinator.observeIntersection(element, callback, options)
}

/**
 * 取消观察元素可见性
 */
export function unobserveIntersection(element: Element): void {
  coordinator.unobserveIntersection(element)
}

/**
 * 获取观察中的元素数量（IntersectionObserver）
 */
export function getIntersectionObservedCount(): number {
  return coordinator.getIntersectionObservedCount()
}

/**
 * 获取 IntersectionObserver 实例数量
 */
export function getIntersectionObserverCount(): number {
  return coordinator.getIntersectionObserverCount()
}

// 页面可见性订阅 API

/**
 * 订阅页面可见性变化
 * 使用统一的监听器，避免每个组件单独添加 visibilitychange 监听
 *
 * @param callback 可见性变化回调 (isVisible: boolean) => void
 * @returns 取消订阅函数
 *
 * @example
 * ```ts
 * // 在组件中使用
 * useEffect(() => {
 *   return onVisibilityChange((isVisible) => {
 *     if (isVisible) {
 *       // 页面可见，恢复轮询
 *       startPolling();
 *     } else {
 *       // 页面隐藏，暂停轮询
 *       stopPolling();
 *     }
 *   });
 * }, []);
 * ```
 */
export function onVisibilityChange(
  callback: (isVisible: boolean) => void,
): () => void {
  return coordinator.onVisibilityChange(callback)
}

/**
 * 获取当前页面可见性状态
 */
export function getPageVisibility(): boolean {
  return coordinator.getPageVisibility()
}

// 空闲任务调度 API

/**
 * 调度空闲任务
 * 在主线程空闲时执行非关键任务，避免阻塞用户交互
 *
 * 特性：
 * - 使用 requestIdleCallback（带 polyfill）
 * - 支持优先级排序
 * - 支持任务去重
 * - 页面不可见时暂停
 *
 * @param id 任务唯一标识（用于去重和取消）
 * @param task 要执行的任务
 * @param options.timeout 超时时间（ms）
 * @param options.priority 优先级（'low' | 'normal' | 'high'）
 * @param options.dedupe 是否去重（默认 true）
 * @returns 取消任务的函数
 *
 * @example
 * ```ts
 * // 调度预加载任务
 * const cancel = scheduleIdleTask('prefetch-images', () => {
 *   prefetchNextPageImages();
 * }, { timeout: 2000, priority: 'low' });
 *
 * // 取消任务
 * cancel();
 * ```
 */
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

/**
 * 取消空闲任务
 */
export function cancelIdleTask(id: string): boolean {
  return coordinator.cancelIdleTask(id)
}

/**
 * 获取待处理的空闲任务数量
 */
export function getIdleTaskCount(): number {
  return coordinator.getIdleTaskCount()
}
