/**
 * 页面就绪状态 Hook
 *
 * 简单的页面就绪状态检测
 * 向后兼容，替代原有的 usePageReady
 */

import { useEffect, useState } from 'react'
import { coordinator } from './coordinator'

/**
 * 获取页面就绪状态
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const isPageReady = usePageReady();
 *
 *   return (
 *     <motion.div
 *       animate={isPageReady ? { opacity: 1 } : { opacity: 0 }}
 *     />
 *   );
 * }
 * ```
 */
export function usePageReady(): boolean {
  const [isReady, setIsReady] = useState(() => coordinator.getPageReadyState())

  useEffect(() => {
    // 如果已就绪，直接返回
    if (coordinator.getPageReadyState()) {
      setIsReady(true)
      return
    }

    // 注册回调
    const unsubscribe = coordinator.onPageReady(() => {
      setIsReady(true)
    })

    return unsubscribe
  }, [])

  return isReady
}

export default usePageReady
