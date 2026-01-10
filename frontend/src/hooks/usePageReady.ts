/**
 * usePageReady Hook
 *
 * 向后兼容的导出，实际使用新的动画协调系统
 * @deprecated 请使用 import { usePageReady } from './animation'
 */

// 重新导出新的实现
// 兼容旧 API
import { coordinator } from './animation'

export { usePageReady as default, usePageReady } from './animation'
export { useStaggerAnimation as useStaggeredDelay } from './animation'

/**
 * @deprecated 使用 coordinator.completePageTransition() 代替
 */
export function markPageAnimationComplete() {
  coordinator.completePageTransition()
}

/**
 * @deprecated 使用 coordinator.startPageTransition() 代替
 */
export function resetPageAnimationState() {
  // 由 coordinator.startPageTransition 自动处理
}

/**
 * @deprecated 使用 coordinator.getPageReadyState() 代替
 */
export function isPageReady(): boolean {
  return coordinator.getPageReadyState()
}
