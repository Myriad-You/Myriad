import { useCallback, useEffect, useState } from 'react'
import { debounce, throttle } from '../utils/performance'

/**
 * 防抖 Hook - 延迟更新值直到指定时间内没有新的变化
 * @param value 要防抖的值
 * @param delay 延迟时间(毫秒)
 * @returns 防抖后的值
 */
export function useDebounce<T>(value: T, delay: number = 300): T {
  const [debouncedValue, setDebouncedValue] = useState<T>(value)

  useEffect(() => {
    const handler = setTimeout(() => {
      setDebouncedValue(value)
    }, delay)

    return () => {
      clearTimeout(handler)
    }
  }, [value, delay])

  return debouncedValue
}

/**
 * 防抖回调Hook - 优化频繁触发的函数调用
 * @param callback 原始回调
 * @param delay 延迟时间(ms)
 * @param deps 依赖数组
 * @returns 防抖后的回调
 */
export function useDebouncedCallback<T extends (...args: any[]) => any>(
  callback: T,
  delay: number = 300,
  deps: React.DependencyList = [],
): T {
  return useCallback(
    debounce(callback, delay) as T,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [delay, ...deps],
  )
}

/**
 * 节流回调Hook - 限制函数执行频率
 * @param callback 原始回调
 * @param limit 限制时间(ms)
 * @param deps 依赖数组
 * @returns 节流后的回调
 */
export function useThrottledCallback<T extends (...args: any[]) => any>(
  callback: T,
  limit: number = 300,
  deps: React.DependencyList = [],
): T {
  return useCallback(
    throttle(callback, limit) as T,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [limit, ...deps],
  )
}

/**
 * 防抖输入Hook - 优化输入框性能
 * 返回受控组件所需的props
 */
export function useDebouncedInput(
  initialValue: string,
  onDebouncedChange: (value: string) => void,
  delay: number = 300,
) {
  const [value, setValue] = useState(initialValue)

  // 创建防抖的onChange
  const debouncedOnChange = useDebouncedCallback(
    (newValue: string) => {
      onDebouncedChange(newValue)
    },
    delay,
    [onDebouncedChange],
  )

  // 处理输入变化
  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => {
      const newValue = e.target.value
      setValue(newValue)
      debouncedOnChange(newValue)
    },
    [debouncedOnChange],
  )

  // 当外部值改变时同步
  useEffect(() => {
    setValue(initialValue)
  }, [initialValue])

  return {
    value,
    onChange: handleChange,
  }
}
