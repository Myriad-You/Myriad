import { useCallback, useRef } from 'react'

/**
 * 节流 Hook - 限制函数在指定时间内最多执行一次
 * @param callback 要节流的回调函数
 * @param delay 节流时间（毫秒）
 * @returns 节流后的函数
 */
export function useThrottle<T extends (...args: any[]) => any>(
  callback: T,
  delay: number = 300,
): (...args: Parameters<T>) => void {
  const lastRun = useRef<number>(Date.now())
  const timeoutRef = useRef<NodeJS.Timeout>()

  return useCallback(
    (...args: Parameters<T>) => {
      const now = Date.now()
      const timeSinceLastRun = now - lastRun.current

      if (timeSinceLastRun >= delay) {
        callback(...args)
        lastRun.current = now
      }
      else {
        if (timeoutRef.current) {
          clearTimeout(timeoutRef.current)
        }
        timeoutRef.current = setTimeout(() => {
          callback(...args)
          lastRun.current = Date.now()
        }, delay - timeSinceLastRun)
      }
    },
    [callback, delay],
  )
}
